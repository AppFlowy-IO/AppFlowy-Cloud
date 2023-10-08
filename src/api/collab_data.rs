use crate::biz;
use crate::component::auth::jwt::UserUuid;
use crate::component::storage_proxy::CollabStorageProxy;
use crate::state::{AppState, Storage};
use actix_web::web::{Data, Json};
use actix_web::Result;
use actix_web::{web, Scope};
use database::collab::CollabStorage;

use database_entity::database_error::DatabaseError;
use database_entity::{
  AFCollabSnapshots, BatchQueryCollabParams, BatchQueryCollabResult, DeleteCollabParams,
  InsertCollabParams, QueryCollabParams, QueryObjectSnapshotParams, QuerySnapshotParams, RawData,
};
use shared_entity::app_error::AppError;
use shared_entity::data::AppResponse;
use shared_entity::error_code::ErrorCode;
use tracing::{debug, instrument};
use tracing_actix_web::RequestId;

pub fn collab_scope() -> Scope {
  web::scope("/api/collab")
    .service(
      web::resource("/")
        .route(web::post().to(create_collab_handler))
        .route(web::get().to(get_collab_handler))
        .route(web::put().to(update_collab_handler))
        .route(web::delete().to(delete_collab_handler)),
    )
    .service(web::resource("list").route(web::get().to(batch_get_collab_handler)))
    .service(web::resource("snapshot").route(web::get().to(retrieve_snapshot_data_handler)))
    .service(web::resource("snapshots").route(web::get().to(retrieve_snapshots_handler)))
}

#[instrument(skip(state, payload), err)]
async fn create_collab_handler(
  user_uuid: UserUuid,
  required_id: RequestId,
  payload: Json<InsertCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  biz::collab::create_collab(&state.pg_pool, &user_uuid, &payload.into_inner()).await?;
  Ok(Json(AppResponse::Ok()))
}

#[instrument(skip(storage, payload), err)]
async fn get_collab_handler(
  user_uuid: UserUuid,
  required_id: RequestId,
  payload: Json<QueryCollabParams>,
  storage: Data<Storage<CollabStorageProxy>>,
) -> Result<Json<AppResponse<RawData>>> {
  // TODO: access control for user_uuid

  let data = storage
    .collab_storage
    .get_collab(payload.into_inner())
    .await
    .map_err(|err| match &err {
      DatabaseError::RecordNotFound => AppError::new(ErrorCode::RecordNotFound, err.to_string()),
      _ => AppError::new(ErrorCode::DatabaseError, err.to_string()),
    })?;

  debug!("Returned data length: {}", data.len());
  Ok(Json(AppResponse::Ok().with_data(data)))
}

#[instrument(skip(storage, payload), err)]
async fn batch_get_collab_handler(
  user_uuid: UserUuid,
  required_id: RequestId,
  payload: Json<BatchQueryCollabParams>,
  storage: Data<Storage<CollabStorageProxy>>,
) -> Result<Json<AppResponse<BatchQueryCollabResult>>> {
  // TODO: access control for user_uuid
  let result = BatchQueryCollabResult(
    storage
      .collab_storage
      .batch_get_collab(payload.into_inner().0)
      .await,
  );
  Ok(Json(AppResponse::Ok().with_data(result)))
}

#[instrument(skip(state, payload), err)]
async fn update_collab_handler(
  user_uuid: UserUuid,
  required_id: RequestId,
  payload: Json<InsertCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  biz::collab::upsert_collab(&state.pg_pool, &user_uuid, &payload.into_inner()).await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(level = "info", skip(state, payload), err)]
async fn delete_collab_handler(
  user_uuid: UserUuid,
  required_id: RequestId,
  payload: Json<DeleteCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  biz::collab::delete_collab(&state.pg_pool, &user_uuid, &payload.into_inner()).await?;
  Ok(AppResponse::Ok().into())
}

async fn retrieve_snapshot_data_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  payload: Json<QuerySnapshotParams>,
) -> Result<Json<AppResponse<RawData>>> {
  let data =
    biz::collab::get_collab_snapshot(&state.pg_pool, &user_uuid, &payload.into_inner()).await?;
  Ok(Json(AppResponse::Ok().with_data(data)))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn retrieve_snapshots_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  payload: Json<QueryObjectSnapshotParams>,
) -> Result<Json<AppResponse<AFCollabSnapshots>>> {
  let data =
    biz::collab::get_all_collab_snapshot(&state.pg_pool, &user_uuid, &payload.into_inner()).await?;
  Ok(Json(AppResponse::Ok().with_data(data)))
}
