use crate::biz;
use crate::component::auth::jwt::UserUuid;
use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::Result;
use actix_web::{web, Scope};
use database_entity::{AFCollabSnapshots, QueryObjectSnapshotParams, QuerySnapshotParams, RawData};
use shared_entity::data::AppResponse;
use shared_entity::dto::{DeleteCollabParams, InsertCollabParams, QueryCollabParams};
use tracing::instrument;

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
  user_uuid: UserUuid,
  payload: Json<InsertCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  biz::collaborate::create_collab(&state.pg_pool, &user_uuid, &payload.into_inner()).await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(skip_all, err)]
async fn get_collab_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  payload: Json<QueryCollabParams>,
) -> Result<Json<AppResponse<RawData>>> {
  let data = biz::collaborate::get_collab_raw(
    &state.pg_pool,
    state.redis_client.clone(),
    &user_uuid,
    payload.into_inner(),
  )
  .await?;
  Ok(Json(AppResponse::Ok().with_data(data)))
}

#[instrument(skip_all, err)]
async fn update_collab_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  payload: Json<InsertCollabParams>,
) -> Result<Json<AppResponse<()>>> {
  biz::collaborate::update_collab(
    &state.pg_pool,
    state.redis_client.clone(),
    &user_uuid,
    &payload.into_inner(),
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(level = "info", skip_all, err)]
async fn delete_collab_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  payload: Json<DeleteCollabParams>,
) -> Result<Json<AppResponse<()>>> {
  biz::collaborate::delete(
    &state.pg_pool,
    state.redis_client.clone(),
    &user_uuid,
    &payload.into_inner(),
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

async fn retrieve_snapshot_data_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  payload: Json<QuerySnapshotParams>,
) -> Result<Json<AppResponse<RawData>>> {
  let data =
    biz::collaborate::get_collab_snapshot(&state.pg_pool, &user_uuid, &payload.into_inner())
      .await?;
  Ok(Json(AppResponse::Ok().with_data(data)))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn retrieve_snapshots_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  payload: Json<QueryObjectSnapshotParams>,
) -> Result<Json<AppResponse<AFCollabSnapshots>>> {
  let data =
    biz::collaborate::get_all_collab_snapshot(&state.pg_pool, &user_uuid, &payload.into_inner())
      .await?;
  Ok(Json(AppResponse::Ok().with_data(data)))
}
