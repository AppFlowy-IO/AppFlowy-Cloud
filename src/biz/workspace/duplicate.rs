use std::{
  collections::{HashMap, HashSet},
  sync::Arc,
};

use actix_web::web::Data;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use collab_database::{
  database::{gen_database_id, gen_row_id, timestamp, Database, DatabaseContext, DatabaseData},
  entity::{CreateDatabaseParams, CreateViewParams},
  rows::CreateRowParams,
  views::OrderObjectPosition,
  workspace_database::WorkspaceDatabase,
};
use collab_document::document::Document;
use collab_entity::{CollabType, EncodedCollab};
use collab_folder::{Folder, RepeatedViewIdentifier, View, ViewIdentifier};
use collab_rt_entity::user::RealtimeUser;
use database::collab::{select_workspace_database_oid, CollabStorage, GetCollabOrigin};
use database_entity::dto::{CollabParams, QueryCollab, QueryCollabResult};
use itertools::Itertools;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
  api::{metrics::AppFlowyWebMetrics, ws::RealtimeServerAddr},
  biz::collab::{
    database::PostgresDatabaseCollabService,
    utils::{collab_from_doc_state, get_latest_collab_encoded, get_latest_collab_folder},
  },
};

use super::page_view::{update_workspace_database_data, update_workspace_folder_data};

#[allow(clippy::too_many_arguments)]
pub async fn duplicate_view_tree_and_collab(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: Arc<CollabAccessControlStorage>,
  pg_pool: &PgPool,
  workspace_id: Uuid,
  view_id: Uuid,
  suffix: &str,
) -> Result<(), AppError> {
  let uid = user.uid;
  let mut folder: Folder =
    get_latest_collab_folder(&collab_storage, GetCollabOrigin::User { uid }, workspace_id).await?;
  let trash_sections: HashSet<String> = folder
    .get_all_trash_sections()
    .iter()
    .map(|s| s.id.clone())
    .collect();
  let views: Vec<View> = folder
    .get_view_recursively(&view_id.to_string())
    .into_iter()
    .filter(|view| !trash_sections.contains(&view.id))
    .collect();
  let duplicate_context = duplicate_views(&views, suffix)?;

  let ws_db_oid = select_workspace_database_oid(pg_pool, &workspace_id)
    .await
    .map_err(|err| {
      AppError::Internal(anyhow::anyhow!(
        "Unable to find workspace database oid for {}: {}",
        workspace_id,
        err
      ))
    })?;
  let encoded_ws_db = get_latest_collab_encoded(
    &collab_storage,
    GetCollabOrigin::User { uid },
    workspace_id,
    ws_db_oid,
    CollabType::WorkspaceDatabase,
  )
  .await
  .map_err(|err| {
    AppError::Internal(anyhow::anyhow!(
      "Unable to get latest workspace database collab {}: {}",
      &ws_db_oid,
      err
    ))
  })?;
  let ws_db_collab =
    collab_from_doc_state(encoded_ws_db.doc_state.to_vec(), &ws_db_oid).map_err(|err| {
      AppError::Internal(anyhow::anyhow!(
        "Unable to decode workspace database collab {}: {}",
        &ws_db_oid,
        err
      ))
    })?;
  let mut ws_db = WorkspaceDatabase::open(ws_db_collab).map_err(|err| {
    AppError::Internal(anyhow::anyhow!(
      "Failed to open workspace database body: {}",
      err
    ))
  })?;

  duplicate_database(
    appflowy_web_metrics,
    server.clone(),
    user.clone(),
    collab_storage.clone(),
    workspace_id,
    &duplicate_context,
    &mut ws_db,
  )
  .await?;

  duplicate_document(
    collab_storage.clone(),
    workspace_id,
    uid,
    &duplicate_context,
  )
  .await?;

  let encoded_folder_update = {
    let mut txn = folder.collab.transact_mut();
    for view in &duplicate_context.duplicated_views {
      folder.body.views.insert(&mut txn, view.clone(), None);
    }
    txn.encode_update_v1()
  };
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    encoded_folder_update,
  )
  .await?;
  Ok(())
}

fn duplicate_database_data_with_context(
  context: &DuplicateContext,
  data: &DatabaseData,
) -> CreateDatabaseParams {
  let database_id = gen_database_id();
  let timestamp = timestamp();

  let create_row_params = data
    .rows
    .iter()
    .map(|row| CreateRowParams {
      id: gen_row_id(),
      database_id: database_id.clone(),
      created_at: timestamp,
      modified_at: timestamp,
      cells: row.cells.clone(),
      height: row.height,
      visibility: row.visibility,
      row_position: OrderObjectPosition::End,
    })
    .collect();

  let create_view_params = data
    .views
    .iter()
    .map(|view| CreateViewParams {
      database_id: database_id.clone(),
      view_id: context
        .view_id_mapping
        .get(&Uuid::parse_str(&view.id).unwrap())
        .cloned()
        .unwrap_or_else(Uuid::new_v4)
        .to_string(),
      name: view.name.clone(),
      layout: view.layout,
      layout_settings: view.layout_settings.clone(),
      filters: view.filters.clone(),
      group_settings: view.group_settings.clone(),
      sorts: view.sorts.clone(),
      field_settings: view.field_settings.clone(),
      created_at: timestamp,
      modified_at: timestamp,
      ..Default::default()
    })
    .collect();

  CreateDatabaseParams {
    database_id,
    rows: create_row_params,
    fields: data.fields.clone(),
    views: create_view_params,
  }
}

async fn duplicate_database(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: Arc<CollabAccessControlStorage>,
  workspace_id: Uuid,
  duplicate_context: &DuplicateContext,
  workspace_database: &mut WorkspaceDatabase,
) -> Result<(), AppError> {
  let uid = user.uid;
  let collab_service = Arc::new(PostgresDatabaseCollabService {
    workspace_id,
    collab_storage: collab_storage.clone(),
  });
  let mut database_id_list: HashSet<String> = HashSet::new();

  for database_view_id in &duplicate_context.database_view_ids {
    let database_id = workspace_database
      .get_database_meta_with_view_id(&database_view_id.to_string())
      .ok_or_else(|| {
        AppError::Internal(anyhow!("Database view id {} not found", database_view_id))
      })?
      .database_id
      .clone();
    database_id_list.insert(database_id);
  }

  for database_id in &database_id_list {
    let database_context = DatabaseContext {
      collab_service: collab_service.clone(),
      notifier: Default::default(),
    };
    let database = Database::open(database_id, database_context)
      .await
      .map_err(|err| AppError::Internal(anyhow::anyhow!("Failed to open database: {}", err)))?;
    let database_data = database.get_database_data().await;
    let params = duplicate_database_data_with_context(duplicate_context, &database_data);
    let database_context = DatabaseContext {
      collab_service: collab_service.clone(),
      notifier: Default::default(),
    };
    let duplicated_database = Database::create_with_view(params, database_context)
      .await
      .map_err(|err| {
        AppError::Internal(anyhow::anyhow!("Failed to duplicate database: {}", err))
      })?;
    let duplicated_view_ids = duplicated_database
      .get_all_database_views_meta()
      .iter()
      .map(|meta| meta.id.clone())
      .collect_vec();
    let encoded_database = duplicated_database
      .encode_database_collabs()
      .await
      .map_err(|err| {
        AppError::Internal(anyhow::anyhow!(
          "Failed to encode database collabs: {}",
          err
        ))
      })?;
    let mut collab_params_list = vec![];
    let database_id = Uuid::parse_str(&duplicated_database.get_database_id())?;
    collab_params_list.push(CollabParams {
      object_id: database_id,
      encoded_collab_v1: encoded_database
        .encoded_database_collab
        .encoded_collab
        .encode_to_bytes()?
        .into(),
      collab_type: CollabType::Database,
      updated_at: None,
    });
    for row in encoded_database.encoded_row_collabs {
      let row_id = Uuid::parse_str(&row.object_id.clone())?;
      collab_params_list.push(CollabParams {
        object_id: row_id,
        encoded_collab_v1: row.encoded_collab.encode_to_bytes()?.into(),
        collab_type: CollabType::DatabaseRow,
        updated_at: None,
      });
    }
    collab_storage
      .batch_insert_new_collab(workspace_id, &uid, collab_params_list)
      .await?;
    let encoded_update = {
      let mut txn = workspace_database.collab.transact_mut();
      workspace_database.body.add_database(
        &mut txn,
        duplicated_database.object_id(),
        duplicated_view_ids,
      );
      txn.encode_update_v1()
    };
    let workspace_database_id = Uuid::parse_str(workspace_database.collab.object_id())?;
    update_workspace_database_data(
      appflowy_web_metrics,
      server.clone(),
      user.clone(),
      workspace_id,
      workspace_database_id,
      encoded_update,
    )
    .await?;
  }
  Ok(())
}

async fn duplicate_document(
  collab_storage: Arc<CollabAccessControlStorage>,
  workspace_id: Uuid,
  uid: i64,
  duplicate_context: &DuplicateContext,
) -> Result<(), AppError> {
  let queries = duplicate_context
    .document_view_ids
    .iter()
    .map(|id| QueryCollab {
      object_id: *id,
      collab_type: CollabType::Document,
    })
    .collect();
  let query_results = collab_storage
    .batch_get_collab(&uid, workspace_id, queries, true)
    .await;
  let mut collab_params_list = vec![];
  for (collab_id, query_result) in query_results {
    match query_result {
      QueryCollabResult::Success { encode_collab_v1 } => {
        let encoded_collab = EncodedCollab::decode_from_bytes(&encode_collab_v1)
          .map_err(|err| AppError::Internal(anyhow::anyhow!("Failed to decode collab: {}", err)))?;
        let new_collab_id = duplicate_context
          .view_id_mapping
          .get(&collab_id)
          .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
              "Failed to find new collab id for {}",
              collab_id
            ))
          })?;
        let new_collab_param =
          duplicate_document_encoded_collab(&collab_id, *new_collab_id, encoded_collab)?;
        collab_params_list.push(new_collab_param);
      },
      QueryCollabResult::Failed { error: _ } => {
        tracing::warn!("Failed to read collab {} during duplication", collab_id);
      },
    }
  }
  collab_storage
    .batch_insert_new_collab(workspace_id, &uid, collab_params_list)
    .await?;
  Ok(())
}

struct DuplicateContext {
  view_id_mapping: HashMap<Uuid, Uuid>,
  duplicated_views: Vec<View>,
  database_view_ids: HashSet<Uuid>,
  document_view_ids: HashSet<Uuid>,
}

fn duplicate_views(views: &[View], suffix: &str) -> Result<DuplicateContext, AppError> {
  let root_parent_id = views
    .first()
    .ok_or(AppError::Internal(anyhow!(
      "No views available for duplication"
    )))?
    .parent_view_id
    .clone();
  let mut view_id_mapping = HashMap::new();
  let mut duplicated_views = vec![];
  let mut database_view_ids = HashSet::new();
  let mut document_view_ids = HashSet::new();
  for view in views {
    let view_id = Uuid::parse_str(&view.id)?;
    let duplicated_view_id = Uuid::new_v4();
    view_id_mapping.insert(view_id, duplicated_view_id);
  }
  for (index, view) in views.iter().enumerate() {
    let view_id = Uuid::parse_str(&view.id)?;
    let orig_parent_view_id = Uuid::parse_str(&view.parent_view_id)?;
    let duplicated_parent_view_id = if view.parent_view_id == root_parent_id {
      orig_parent_view_id
    } else {
      view_id_mapping
        .get(&orig_parent_view_id)
        .cloned()
        .ok_or(AppError::Internal(anyhow::anyhow!(
          "Failed to find duplicated parent view id {}",
          view.parent_view_id
        )))?
    };
    let mut duplicated_view = view.clone();
    let mut duplicated_children = vec![];
    for child in view.children.items.iter() {
      let child_id = Uuid::parse_str(&child.id)?;
      let new_view_id = view_id_mapping.get(&child_id).cloned();
      if let Some(view_id) = new_view_id {
        duplicated_children.push(ViewIdentifier {
          id: view_id.to_string(),
        });
      }
    }
    duplicated_view.id = view_id_mapping
      .get(&view_id)
      .cloned()
      .ok_or(AppError::Internal(anyhow::anyhow!(
        "Failed to find duplicated view id {}",
        view.id
      )))?
      .to_string();
    duplicated_view.parent_view_id = duplicated_parent_view_id.to_string();
    if index == 0 {
      duplicated_view.name = format!("{}{}", duplicated_view.name, suffix);
    }
    duplicated_view.created_at = timestamp();
    duplicated_view.is_favorite = false;
    duplicated_view.last_edited_time = 0;
    duplicated_view.children = RepeatedViewIdentifier {
      items: duplicated_children,
    };

    duplicated_views.push(duplicated_view);
    match &view.layout {
      layout if layout.is_document() => {
        document_view_ids.insert(view_id);
      },
      layout if layout.is_database() => {
        database_view_ids.insert(view_id);
      },
      _ => (),
    }
  }
  Ok(DuplicateContext {
    view_id_mapping,
    duplicated_views,
    database_view_ids,
    document_view_ids,
  })
}

fn duplicate_document_encoded_collab(
  orig_object_id: &Uuid,
  new_object_id: Uuid,
  encoded_collab: EncodedCollab,
) -> Result<CollabParams, AppError> {
  let collab = collab_from_doc_state(encoded_collab.doc_state.to_vec(), orig_object_id)?;
  let document = Document::open(collab).unwrap();
  let data = document.get_document_data().unwrap();
  let duplicated_document = Document::create(&new_object_id.to_string(), data)
    .map_err(|err| AppError::Internal(anyhow::anyhow!("Failed to create document: {}", err)))?;
  let encoded_collab: EncodedCollab = duplicated_document
    .encode_collab_v1(|c| CollabType::Document.validate_require_data(c))
    .map_err(|err| {
      AppError::Internal(anyhow::anyhow!("Failed to encode document collab: {}", err))
    })?;
  Ok(CollabParams {
    object_id: new_object_id,
    encoded_collab_v1: encoded_collab.encode_to_bytes()?.into(),
    collab_type: CollabType::Document,
    updated_at: None,
  })
}
