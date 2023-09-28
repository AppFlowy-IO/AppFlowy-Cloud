use crate::component::storage_proxy::CollabStorageProxy;
use crate::state::Storage;
use actix_web::web::{Data, Json};
use actix_web::Result;
use actix_web::{web, Scope};
use database::collab::CollabStorage;
use database::error::StorageError;
use database_entity::{
  AFCollabSnapshots, DeleteCollabParams, InsertCollabParams, QueryCollabParams,
  QueryObjectSnapshotParams, QuerySnapshotParams, RawData,
};
use shared_entity::data::AppResponse;
use shared_entity::error::AppError;
use shared_entity::error_code::ErrorCode;
use tracing::{debug, instrument};
use validator::Validate;

pub fn collab_scope() -> Scope {
  web::scope("/api/collab")
    .service(
      web::resource("/")
        .route(web::post().to(create_collab_handler))
        .route(web::get().to(get_collab_handler))
        .route(web::put().to(update_collab_handler))
        .route(web::delete().to(delete_collab_handler)),
    )
    .service(web::resource("snapshot").route(web::get().to(retrieve_snapshot_data_handler)))
    .service(web::resource("snapshots").route(web::get().to(retrieve_snapshots_handler)))
}

#[instrument(skip_all, err)]
async fn create_collab_handler(
  payload: Json<InsertCollabParams>,
  storage: Data<Storage<CollabStorageProxy>>,
) -> Result<Json<AppResponse<()>>> {
  let params = payload.into_inner();
  if storage.collab_storage.is_exist(&params.object_id).await {
    return Ok(Json(
      AppResponse::Ok()
        .with_code(ErrorCode::RecordAlreadyExists)
        .with_message(format!("Collab:{} already exists", params.object_id)),
    ));
  }

  storage
    .collab_storage
    .insert_collab(params)
    .await
    .map_err(|err| AppError::new(ErrorCode::StorageError, err.to_string()))?;
  Ok(Json(AppResponse::Ok()))
}

#[instrument(skip(storage), err)]
async fn get_collab_handler(
  payload: Json<QueryCollabParams>,
  storage: Data<Storage<CollabStorageProxy>>,
) -> Result<Json<AppResponse<RawData>>> {
  let data = storage
    .collab_storage
    .get_collab(payload.into_inner())
    .await
    .map_err(|err| match &err {
      StorageError::RecordNotFound => AppError::new(ErrorCode::RecordNotFound, err.to_string()),
      _ => AppError::new(ErrorCode::StorageError, err.to_string()),
    })?;

  debug!("Returned data length: {}", data.len());
  Ok(Json(AppResponse::Ok().with_data(data)))
}

#[instrument(skip_all, err)]
async fn update_collab_handler(
  payload: Json<InsertCollabParams>,
  storage: Data<Storage<CollabStorageProxy>>,
) -> Result<Json<AppResponse<()>>> {
  let params = payload.into_inner();
  storage
    .collab_storage
    .insert_collab(params)
    .await
    .map_err(|err| AppError::new(ErrorCode::StorageError, err.to_string()))?;
  Ok(Json(AppResponse::Ok()))
}

#[instrument(level = "info", skip_all, err)]
async fn delete_collab_handler(
  payload: Json<DeleteCollabParams>,
  storage: Data<Storage<CollabStorageProxy>>,
) -> Result<Json<AppResponse<()>>> {
  let params = payload.into_inner();
  params.validate().map_err(AppError::from)?;

  storage
    .collab_storage
    .delete_collab(&params.object_id)
    .await
    .map_err(|err| AppError::new(ErrorCode::StorageError, err.to_string()))?;
  Ok(Json(AppResponse::Ok()))
}

async fn retrieve_snapshot_data_handler(
  payload: Json<QuerySnapshotParams>,
  storage: Data<Storage<CollabStorageProxy>>,
) -> Result<Json<AppResponse<RawData>>> {
  let data = storage
    .collab_storage
    .get_snapshot_data(payload.into_inner())
    .await
    .map_err(|err| match &err {
      StorageError::RecordNotFound => AppError::new(ErrorCode::RecordNotFound, err.to_string()),
      _ => AppError::new(ErrorCode::StorageError, err.to_string()),
    })?;
  Ok(Json(AppResponse::Ok().with_data(data)))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn retrieve_snapshots_handler(
  payload: Json<QueryObjectSnapshotParams>,
  storage: Data<Storage<CollabStorageProxy>>,
) -> Result<Json<AppResponse<AFCollabSnapshots>>> {
  let data = storage
    .collab_storage
    .get_all_snapshots(payload.into_inner())
    .await
    .map_err(|err| match &err {
      StorageError::RecordNotFound => AppError::new(ErrorCode::RecordNotFound, err.to_string()),
      _ => AppError::new(ErrorCode::StorageError, err.to_string()),
    })?;
  Ok(Json(AppResponse::Ok().with_data(data)))
}
