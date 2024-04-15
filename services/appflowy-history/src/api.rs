use crate::application::AppState;
use crate::biz::history::get_snapshots;
use crate::error::HistoryError;
use crate::response::APIResponse;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::Router;
use collab_entity::CollabType;
use database::history::model::RepeatedSnapshotMeta;

pub fn router() -> Router<AppState> {
  Router::new()
    .route(
      "/api/snapshot/:workspace_id/:object_id",
      get(get_snapshot_handler),
    )
    .route(
      "/api/history/:workspace_id/:object_id",
      get(get_history_handler),
    )
}

pub async fn get_snapshot_handler(
  Path((_workspace_id, object_id)): Path<(String, String)>,
  collab_type: Query<CollabType>,
  State(state): State<AppState>,
) -> axum::response::Result<APIResponse<RepeatedSnapshotMeta>, HistoryError> {
  let data = get_snapshots(&object_id, &collab_type, &state.pg_pool).await?;
  Ok(APIResponse::new(data))
}

pub async fn get_history_handler(
  Path((_workspace_id, _object_id)): Path<(String, String)>,
  State(_state): State<AppState>,
) -> axum::response::Result<APIResponse<()>, HistoryError> {
  Ok(APIResponse::new(()).with_message("hello".to_string()))
}
