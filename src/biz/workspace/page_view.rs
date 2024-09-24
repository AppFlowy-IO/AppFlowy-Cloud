use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use app_error::ErrorCode;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use chrono::DateTime;
use collab::core::collab::Collab;
use collab_database::{
  database::DatabaseBody, rows::RowId, workspace_database::WorkspaceDatabaseBody,
};
use collab_entity::{CollabType, EncodedCollab};
use collab_folder::CollabOrigin;
use database::collab::{select_workspace_database_oid, CollabStorage, GetCollabOrigin};
use database::publish::select_published_view_ids_for_workspace;
use database::user::select_web_user_from_uid;
use database_entity::dto::{QueryCollab, QueryCollabResult};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use shared_entity::dto::workspace_dto::{FolderView, PageCollab, PageCollabData};
use shared_entity::response::AppResponseError;
use sqlx::PgPool;
use uuid::Uuid;

use crate::biz::collab::folder_view::{
  parse_extra_field_as_json, to_dto_view_icon, to_view_layout,
};
use crate::biz::collab::{
  folder_view::view_is_space,
  ops::{get_latest_collab_encoded, get_latest_collab_folder},
};

use super::publish_dup::collab_from_doc_state;

pub async fn get_page_view_collab(
  pg_pool: &PgPool,
  collab_access_control_storage: Arc<CollabAccessControlStorage>,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<PageCollab, AppResponseError> {
  let folder = get_latest_collab_folder(
    collab_access_control_storage.clone(),
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
  )
  .await?;
  let view = folder.get_view(view_id).ok_or(AppResponseError::new(
    ErrorCode::InvalidFolderView,
    format!("View {} not found", view_id),
  ))?;

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
    layout: to_view_layout(&view.layout),
    created_at: DateTime::from_timestamp(view.created_at, 0).unwrap_or_default(),
    last_edited_time: DateTime::from_timestamp(view.last_edited_time, 0).unwrap_or_default(),
    extra: view.extra.as_ref().map(|e| parse_extra_field_as_json(e)),
    children: vec![],
  };
  let page_collab_data = match view.layout {
    collab_folder::ViewLayout::Document => {
      get_page_collab_data_for_document(
        collab_access_control_storage.clone(),
        uid,
        workspace_id,
        view_id,
      )
      .await
    },
    collab_folder::ViewLayout::Grid
    | collab_folder::ViewLayout::Board
    | collab_folder::ViewLayout::Calendar => {
      get_page_collab_data_for_database(
        pg_pool,
        collab_access_control_storage.clone(),
        uid,
        workspace_id,
        view_id,
      )
      .await
    },
    collab_folder::ViewLayout::Chat => Err(AppResponseError::new(
      ErrorCode::InvalidRequest,
      "Page view for AI chat is not supported at the moment",
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
  collab_access_control_storage: Arc<CollabAccessControlStorage>,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<PageCollabData, AppResponseError> {
  let ws_db_oid = select_workspace_database_oid(pg_pool, &workspace_id).await?;
  let ws_db = get_latest_collab_encoded(
    collab_access_control_storage.clone(),
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
    &ws_db_oid,
    CollabType::WorkspaceDatabase,
  )
  .await?;
  let mut ws_db_collab = collab_from_doc_state(ws_db.doc_state.to_vec(), &ws_db_oid)?;
  let ws_db_body = WorkspaceDatabaseBody::open(&mut ws_db_collab);
  let db_oid = {
    let txn = ws_db_collab.transact();
    ws_db_body
      .get_database_meta_with_view_id(&txn, view_id)
      .ok_or(AppResponseError::new(
        ErrorCode::NoRequiredData,
        format!("Database view {} not found", view_id),
      ))?
      .database_id
  };
  let db = get_latest_collab_encoded(
    collab_access_control_storage.clone(),
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
    AppResponseError::new(
      ErrorCode::Internal,
      format!(
        "Unable to create collab from object id {}: {}",
        &db_oid, err
      ),
    )
  })?;
  let db_body = DatabaseBody::from_collab(&db_collab).unwrap();
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
  collab_access_control_storage: Arc<CollabAccessControlStorage>,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<PageCollabData, AppResponseError> {
  let collab = get_latest_collab_encoded(
    collab_access_control_storage.clone(),
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
