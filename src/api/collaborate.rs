use crate::state::Storage;
use actix_web::web::{Data, Json};
use actix_web::Result;
use actix_web::{web, Scope};
use shared_entity::data::AppResponse;
use shared_entity::error::AppError;
use shared_entity::server_error::ErrorCode;
use storage::collab::{CollabPostgresDBStorageImpl, CollabStorage, RawData};
use storage::entities::{DeleteCollabParams, InsertCollabParams, QueryCollabParams};
use storage::error::StorageError;
use tracing::instrument;
use validator::Validate;

pub fn collab_scope() -> Scope {
  web::scope("/api").service(
    web::resource("collab")
      .route(web::post().to(create_collab_handler))
      .route(web::get().to(retrieve_collab_handler))
      .route(web::put().to(update_collab_handler))
      .route(web::delete().to(delete_collab_handler)),
  )
}

#[instrument(level = "debug", skip_all, err)]
async fn create_collab_handler(
  payload: Json<InsertCollabParams>,
  storage: Data<Storage<CollabPostgresDBStorageImpl>>,
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

async fn retrieve_collab_handler(
  payload: Json<QueryCollabParams>,
  storage: Data<Storage<CollabPostgresDBStorageImpl>>,
) -> Result<Json<AppResponse<RawData>>> {
  let data = storage
    .collab_storage
    .get_collab(payload.into_inner())
    .await
    .map_err(|err| match &err {
      StorageError::RecordNotFound => AppError::new(ErrorCode::RecordNotFound, err.to_string()),
      _ => AppError::new(ErrorCode::StorageError, err.to_string()),
    })?;
  Ok(Json(AppResponse::Ok().with_data(data)))
}

async fn update_collab_handler(
  payload: Json<InsertCollabParams>,
  storage: Data<Storage<CollabPostgresDBStorageImpl>>,
) -> Result<Json<AppResponse<()>>> {
  let params = payload.into_inner();
  storage
    .collab_storage
    .insert_collab(params)
    .await
    .map_err(|err| AppError::new(ErrorCode::StorageError, err.to_string()))?;
  Ok(Json(AppResponse::Ok()))
}

async fn delete_collab_handler(
  payload: Json<DeleteCollabParams>,
  storage: Data<Storage<CollabPostgresDBStorageImpl>>,
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
