use crate::state::AppState;
use actix_web::web::Data;
use actix_web::{web, Scope};

use anyhow::anyhow;
use app_error::AppError;
use shared_entity::dto::history_dto::{RepeatedSnapshotMeta, SnapshotMeta};
use shared_entity::response::{AppResponse, JsonAppResponse};
use std::collections::HashMap;
use tonic_proto::history::SnapshotRequestPb;

pub fn history_scope() -> Scope {
  web::scope("/api/history/{workspace_id}")
    .service(web::resource("/{object_id}/{collab_type}").route(web::get().to(get_snapshot_handler)))
}

async fn get_snapshot_handler(
  path: web::Path<(String, String, i32)>,
  _query: web::Query<HashMap<String, String>>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<RepeatedSnapshotMeta>> {
  let (workspace_id, object_id, collab_type) = path.into_inner();

  let request = SnapshotRequestPb {
    workspace_id,
    object_id,
    collab_type,
  };

  if let Some(history_client) = state.grpc_history_client.lock().await.as_mut() {
    let items = history_client
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
  } else {
    let err = AppError::Internal(anyhow!("history service is not available".to_string()));
    Ok(AppResponse::from(err).into())
  }
}
