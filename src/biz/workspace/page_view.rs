use anyhow::anyhow;
use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use chrono::DateTime;
use collab::core::collab::Collab;
use collab_database::workspace_database::{NoPersistenceDatabaseCollabService, WorkspaceDatabase};
use collab_database::{database::DatabaseBody, rows::RowId};
use collab_document::document::Document;
use collab_document::document_data::default_document_data;
use collab_entity::{CollabType, EncodedCollab};
use collab_folder::hierarchy_builder::NestedChildViewBuilder;
use collab_folder::{CollabOrigin, Folder};
use database::collab::{select_workspace_database_oid, CollabStorage, GetCollabOrigin};
use database::publish::select_published_view_ids_for_workspace;
use database::user::select_web_user_from_uid;
use database_entity::dto::{CollabParams, QueryCollab, QueryCollabParams, QueryCollabResult};
use itertools::Itertools;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use shared_entity::dto::workspace_dto::{FolderView, Page, PageCollab, PageCollabData, ViewLayout};
use sqlx::{PgPool, Transaction};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;
use yrs::updates::decoder::Decode;
use yrs::Update;

use crate::api::metrics::AppFlowyWebMetrics;
use crate::biz::collab::folder_view::{
  parse_extra_field_as_json, to_dto_view_icon, to_dto_view_layout,
};
use crate::biz::collab::{
  folder_view::view_is_space,
  ops::{get_latest_collab_encoded, get_latest_collab_folder},
};

use super::ops::{broadcast_update, collab_from_doc_state};

struct FolderUpdate {
  pub updated_encoded_collab: Vec<u8>,
  pub encoded_updates: Vec<u8>,
}

pub async fn create_page(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  parent_view_id: &str,
  view_layout: &ViewLayout,
) -> Result<Page, AppError> {
  if *view_layout != ViewLayout::Document {
    return Err(AppError::InvalidRequest(
      "Only document layout is supported for page creation".to_string(),
    ));
  }
  create_document_page(pg_pool, collab_storage, uid, workspace_id, parent_view_id).await
}

fn prepare_default_document_collab_param() -> Result<CollabParams, AppError> {
  let object_id = Uuid::new_v4().to_string();
  let document_data = default_document_data(&object_id);
  let document = Document::create(&object_id, document_data)
    .map_err(|err| AppError::Internal(anyhow!("Failed to create default document: {}", err)))?;
  let encoded_collab_v1 = document
    .encode_collab()
    .map_err(|err| AppError::Internal(anyhow!("Failed to encode default document: {}", err)))?
    .encode_to_bytes()?;
  Ok(CollabParams {
    object_id: object_id.clone(),
    encoded_collab_v1: encoded_collab_v1.into(),
    collab_type: CollabType::Document,
    embeddings: None,
  })
}

async fn add_new_view_to_folder(
  uid: i64,
  parent_view_id: &str,
  view_id: &str,
  folder: &mut Folder,
) -> Result<FolderUpdate, AppError> {
  let encoded_update = {
    let view = NestedChildViewBuilder::new(uid, parent_view_id.to_string())
      .with_view_id(view_id)
      .build()
      .view;
    let mut txn = folder.collab.transact_mut();
    folder.body.views.insert(&mut txn, view, None);
    txn.encode_update_v1()
  };
  Ok(FolderUpdate {
    updated_encoded_collab: folder_to_encoded_collab(folder)?,
    encoded_updates: encoded_update,
  })
}

async fn move_view_to_trash(view_id: &str, folder: &mut Folder) -> Result<FolderUpdate, AppError> {
  let mut current_view_and_descendants = folder
    .get_views_belong_to(view_id)
    .iter()
    .map(|v| v.id.clone())
    .collect_vec();
  current_view_and_descendants.push(view_id.to_string());

  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    current_view_and_descendants.iter().for_each(|view_id| {
      folder.body.views.update_view(&mut txn, view_id, |update| {
        update.set_favorite(false).set_trash(true).done()
      });
    });
    txn.encode_update_v1()
  };

  Ok(FolderUpdate {
    updated_encoded_collab: folder_to_encoded_collab(folder)?,
    encoded_updates: encoded_update,
  })
}

fn folder_to_encoded_collab(folder: &Folder) -> Result<Vec<u8>, AppError> {
  let collab_type = CollabType::Folder;
  let encoded_folder_collab = folder
    .encode_collab_v1(|collab| collab_type.validate_require_data(collab))
    .map_err(|err| AppError::Internal(anyhow!("Failed to encode workspace folder: {}", err)))?;
  encoded_folder_collab.encode_to_bytes().map_err(|err| {
    AppError::Internal(anyhow!(
      "Failed to encode workspace folder to bytes: {}",
      err
    ))
  })
}

async fn insert_and_broadcast_workspace_folder_update(
  uid: i64,
  workspace_id: Uuid,
  folder_update: FolderUpdate,
  collab_storage: &CollabAccessControlStorage,
  transaction: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<(), AppError> {
  let params = CollabParams {
    object_id: workspace_id.to_string(),
    encoded_collab_v1: folder_update.updated_encoded_collab.into(),
    collab_type: CollabType::Folder,
    embeddings: None,
  };
  let action_description = format!("Update workspace folder: {}", workspace_id);
  collab_storage
    .insert_new_collab_with_transaction(
      &workspace_id.to_string(),
      &uid,
      params,
      transaction,
      &action_description,
    )
    .await?;
  broadcast_update(
    collab_storage,
    &workspace_id.to_string(),
    folder_update.encoded_updates.clone(),
  )
  .await?;
  Ok(())
}

async fn create_document_page(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  parent_view_id: &str,
) -> Result<Page, AppError> {
  let default_document_collab_params = prepare_default_document_collab_param()?;
  let view_id = default_document_collab_params.object_id.clone();
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin, &workspace_id.to_string()).await?;
  let folder_update = add_new_view_to_folder(uid, parent_view_id, &view_id, &mut folder).await?;
  let mut transaction = pg_pool.begin().await?;
  let action = format!("Create new collab: {}", view_id);
  collab_storage
    .insert_new_collab_with_transaction(
      &workspace_id.to_string(),
      &uid,
      default_document_collab_params,
      &mut transaction,
      &action,
    )
    .await?;
  insert_and_broadcast_workspace_folder_update(
    uid,
    workspace_id,
    folder_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  transaction.commit().await?;
  Ok(Page { view_id })
}

pub async fn move_page_to_trash(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin, &workspace_id.to_string()).await?;
  let trash_info = folder.get_my_trash_info();
  if trash_info.into_iter().any(|info| info.id == view_id) {
    return Ok(());
  }
  let folder_update = move_view_to_trash(view_id, &mut folder).await?;
  let mut transaction = pg_pool.begin().await?;
  insert_and_broadcast_workspace_folder_update(
    uid,
    workspace_id,
    folder_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  transaction.commit().await?;
  Ok(())
}

pub async fn get_page_view_collab(
  pg_pool: &PgPool,
  collab_access_control_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<PageCollab, AppError> {
  let folder = get_latest_collab_folder(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
  )
  .await?;
  let view = folder
    .get_view(view_id)
    .ok_or(AppError::InvalidFolderView(format!(
      "View {} not found",
      view_id
    )))?;

  let owner = match view.created_by {
    Some(uid) => Some(select_web_user_from_uid(pg_pool, uid).await?),
    None => None,
  };
  let last_editor = match view.last_edited_by {
    Some(uid) => Some(select_web_user_from_uid(pg_pool, uid).await?),
    None => None,
  };
  let publish_view_ids = select_published_view_ids_for_workspace(pg_pool, workspace_id).await?;
  let publish_view_ids: HashSet<String> = publish_view_ids
    .into_iter()
    .map(|id| id.to_string())
    .collect();
  let folder_view = FolderView {
    view_id: view_id.to_string(),
    name: view.name.clone(),
    icon: view
      .icon
      .as_ref()
      .map(|icon| to_dto_view_icon(icon.clone())),
    is_space: view_is_space(&view),
    is_private: false,
    is_published: publish_view_ids.contains(view_id),
    layout: to_dto_view_layout(&view.layout),
    created_at: DateTime::from_timestamp(view.created_at, 0).unwrap_or_default(),
    last_edited_time: DateTime::from_timestamp(view.last_edited_time, 0).unwrap_or_default(),
    extra: view.extra.as_ref().map(|e| parse_extra_field_as_json(e)),
    children: vec![],
  };
  let page_collab_data = match view.layout {
    collab_folder::ViewLayout::Document => {
      get_page_collab_data_for_document(collab_access_control_storage, uid, workspace_id, view_id)
        .await
    },
    collab_folder::ViewLayout::Grid
    | collab_folder::ViewLayout::Board
    | collab_folder::ViewLayout::Calendar => {
      get_page_collab_data_for_database(
        pg_pool,
        collab_access_control_storage,
        uid,
        workspace_id,
        view_id,
      )
      .await
    },
    collab_folder::ViewLayout::Chat => Err(AppError::InvalidRequest(
      "Page view for AI chat is not supported at the moment".to_string(),
    )),
  }?;

  let page_collab = PageCollab {
    view: folder_view,
    data: page_collab_data,
    owner,
    last_editor,
  };

  Ok(page_collab)
}

async fn get_page_collab_data_for_database(
  pg_pool: &PgPool,
  collab_access_control_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<PageCollabData, AppError> {
  let ws_db_oid = select_workspace_database_oid(pg_pool, &workspace_id).await?;
  let ws_db = get_latest_collab_encoded(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
    &ws_db_oid,
    CollabType::WorkspaceDatabase,
  )
  .await?;
  let ws_db_collab = collab_from_doc_state(ws_db.doc_state.to_vec(), &ws_db_oid)?;
  let ws_db_body = WorkspaceDatabase::open(ws_db_collab).map_err(|err| {
    AppError::Internal(anyhow!("Failed to open workspace database body: {}", err))
  })?;
  let db_oid = {
    ws_db_body
      .get_database_meta_with_view_id(view_id)
      .ok_or(AppError::NoRequiredData(format!(
        "Database view {} not found",
        view_id
      )))?
      .database_id
  };
  let db = get_latest_collab_encoded(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
    &db_oid,
    CollabType::Database,
  )
  .await?;
  let db_collab = Collab::new_with_source(
    CollabOrigin::Server,
    &db_oid,
    db.clone().into(),
    vec![],
    false,
  )
  .map_err(|err| {
    AppError::Internal(anyhow!(
      "Unable to create collab from object id {}: {}",
      &db_oid,
      err
    ))
  })?;
  let db_body = DatabaseBody::from_collab(
    &db_collab,
    Arc::new(NoPersistenceDatabaseCollabService),
    None,
  )
  .ok_or_else(|| AppError::RecordNotFound("no database body found".to_string()))?;
  let inline_view_id = {
    let txn = db_collab.transact();
    db_body.get_inline_view_id(&txn)
  };
  let row_ids: Vec<RowId> = {
    let txn = db_collab.transact();
    db_body
      .views
      .get_row_orders(&txn, &inline_view_id)
      .iter()
      .map(|ro| ro.id.clone())
      .collect()
  };
  let queries: Vec<QueryCollab> = row_ids
    .iter()
    .map(|row_id| QueryCollab {
      object_id: row_id.to_string(),
      collab_type: CollabType::DatabaseRow,
    })
    .collect();
  let row_query_collab_results = collab_access_control_storage
    .batch_get_collab(&uid, queries, true)
    .await;
  let row_data = tokio::task::spawn_blocking(move || {
    let row_collabs: HashMap<String, Vec<u8>> = row_query_collab_results
      .into_par_iter()
      .filter_map(|(row_id, query_collab_result)| match query_collab_result {
        QueryCollabResult::Success { encode_collab_v1 } => {
          let decoded_result = EncodedCollab::decode_from_bytes(&encode_collab_v1);
          match decoded_result {
            Ok(decoded) => Some((row_id, decoded.doc_state.to_vec())),
            Err(err) => {
              tracing::error!("Failed to decode collab for row {}: {}", row_id, err);
              None
            },
          }
        },
        QueryCollabResult::Failed { error } => {
          tracing::error!("Failed to get collab: {:?}", error);
          None
        },
      })
      .collect();
    row_collabs
  })
  .await?;

  Ok(PageCollabData {
    encoded_collab: db.doc_state.to_vec(),
    row_data,
  })
}

async fn get_page_collab_data_for_document(
  collab_access_control_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<PageCollabData, AppError> {
  let collab = get_latest_collab_encoded(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
    view_id,
    CollabType::Document,
  )
  .await?;
  Ok(PageCollabData {
    encoded_collab: collab.doc_state.clone().to_vec(),
    row_data: HashMap::default(),
  })
}

pub async fn update_page_collab_data(
  collab_access_control_storage: Arc<CollabAccessControlStorage>,
  appflowy_web_metrics: Arc<AppFlowyWebMetrics>,
  uid: i64,
  workspace_id: Uuid,
  object_id: Uuid,
  collab_type: CollabType,
  doc_state: &[u8],
) -> Result<(), AppError> {
  let param = QueryCollabParams {
    workspace_id: workspace_id.to_string(),
    inner: QueryCollab {
      object_id: object_id.to_string(),
      collab_type: collab_type.clone(),
    },
  };
  let encode_collab = collab_access_control_storage
    .get_encode_collab(GetCollabOrigin::User { uid }, param, true)
    .await?;
  let mut collab = collab_from_doc_state(encode_collab.doc_state.to_vec(), &object_id.to_string())?;
  appflowy_web_metrics.record_update_size_bytes(doc_state.len());
  let update = Update::decode_v1(doc_state).map_err(|e| {
    appflowy_web_metrics.incr_decoding_failure_count(1);
    AppError::InvalidRequest(format!("Failed to decode update: {}", e))
  })?;
  collab.apply_update(update).map_err(|e| {
    appflowy_web_metrics.incr_apply_update_failure_count(1);
    AppError::InvalidRequest(format!("Failed to apply update: {}", e))
  })?;
  let updated_encoded_collab = collab
    .encode_collab_v1(|c| collab_type.validate_require_data(c))
    .map_err(|e| AppError::Internal(anyhow!("Failed to encode collab: {}", e)))?
    .encode_to_bytes()?;
  let params = CollabParams {
    object_id: object_id.to_string(),
    collab_type: collab_type.clone(),
    encoded_collab_v1: updated_encoded_collab.into(),
    embeddings: None,
  };
  collab_access_control_storage
    .queue_insert_or_update_collab(&workspace_id.to_string(), &uid, params, true)
    .await?;
  broadcast_update(
    &collab_access_control_storage,
    &object_id.to_string(),
    doc_state.to_vec(),
  )
  .await?;
  Ok(())
}
