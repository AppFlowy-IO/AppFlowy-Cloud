use crate::state::AppState;
use actix_web::web::Data;
use actix_web::{web, Scope};

use anyhow::anyhow;
use app_error::AppError;
use shared_entity::dto::history_dto::{HistoryState, RepeatedSnapshotMeta, SnapshotMeta};
use shared_entity::response::{AppResponse, JsonAppResponse};

use tonic_proto::history::SnapshotRequestPb;

pub fn history_scope() -> Scope {
  web::scope("/api/history/{workspace_id}")
    .service(web::resource("/{object_id}/{collab_type}").route(web::get().to(get_snapshot_handler)))
    .service(
      web::resource("/{object_id}/{collab_type}/latest")
        .route(web::get().to(get_latest_history_handler)),
    )
}

async fn get_snapshot_handler(
  path: web::Path<(String, String, i32)>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<RepeatedSnapshotMeta>> {
  let (workspace_id, object_id, collab_type) = path.into_inner();

  let request = SnapshotRequestPb {
    workspace_id,
    object_id,
    collab_type,
  };

  let items = state
    .grpc_history_client
    .lock()
    .await
    .get_snapshots(request)
    .await
    .map_err(|err| AppError::Internal(anyhow!(err.to_string())))?
    .into_inner()
    .items
    .into_iter()
    .map(|item| SnapshotMeta {
      oid: item.oid,
      snapshot: item.snapshot,
      snapshot_version: item.snapshot_version,
      created_at: item.created_at,
    })
    .collect::<Vec<_>>();

  Ok(
    AppResponse::Ok()
      .with_data(RepeatedSnapshotMeta { items })
      .into(),
  )
}

async fn get_latest_history_handler(
  path: web::Path<(String, String, i32)>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<HistoryState>> {
  let (workspace_id, object_id, collab_type) = path.into_inner();
  let request = SnapshotRequestPb {
    workspace_id,
    object_id,
    collab_type,
  };

  let pb = state
    .grpc_history_client
    .lock()
    .await
    .get_latest_history(request)
    .await
    .map_err(|err| AppError::Internal(anyhow!(err.to_string())))?
    .into_inner();

  Ok(
    AppResponse::Ok()
      .with_data(HistoryState {
        object_id: pb.object_id,
        doc_state: pb.doc_state,
        doc_state_version: pb.doc_state_version,
      })
      .into(),
  )
}
