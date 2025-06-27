use actix_web::{
  web::{self, Data, Json},
  Result, Scope,
};

use database_entity::dto::{
  AccessRequestMinimal, ApproveAccessRequestParams, CreateAccessRequestParams,
};
use shared_entity::{
  dto::access_request_dto::AccessRequest,
  response::{AppResponse, JsonAppResponse},
};
use uuid::Uuid;

use crate::{
  biz::{
    access_request::ops::{
      approve_or_reject_access_request, create_access_request, get_access_request,
    },
    authentication::jwt::UserUuid,
  },
  state::AppState,
};

pub fn access_request_scope() -> Scope {
  web::scope("/api/access-request")
    .service(web::resource("").route(web::post().to(post_access_request_handler)))
    .service(web::resource("/{request_id}").route(web::get().to(get_access_request_handler)))
    .service(
      web::resource("/{request_id}/approve")
        .route(web::post().to(post_approve_access_request_handler)),
    )
}

async fn get_access_request_handler(
  uuid: UserUuid,
  access_request_id: web::Path<Uuid>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AccessRequest>> {
  let access_request_id = access_request_id.into_inner();
  let uid = state.user_cache.get_user_uid(&uuid).await?;
  let access_request =
    get_access_request(&state.pg_pool, &state.ws_server, access_request_id, uid).await?;
  Ok(Json(AppResponse::Ok().with_data(access_request)))
}

async fn post_access_request_handler(
  uuid: UserUuid,
  create_access_request_params: Json<CreateAccessRequestParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AccessRequestMinimal>> {
  let uid = state.user_cache.get_user_uid(&uuid).await?;
  let workspace_id = create_access_request_params.workspace_id;
  let view_id = create_access_request_params.view_id;
  let request_id = create_access_request(
    &state.pg_pool,
    state.mailer.clone(),
    &state.config.appflowy_web_url,
    workspace_id,
    view_id,
    uid,
  )
  .await?;
  let access_request = AccessRequestMinimal {
    request_id,
    workspace_id,
    requester_id: *uuid,
    view_id,
  };
  Ok(Json(AppResponse::Ok().with_data(access_request)))
}

async fn post_approve_access_request_handler(
  uuid: UserUuid,
  access_request_id: web::Path<Uuid>,
  approve_access_request_params: Json<ApproveAccessRequestParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  let uid = state.user_cache.get_user_uid(&uuid).await?;
  let access_request_id = access_request_id.into_inner();
  let is_approved = approve_access_request_params.is_approved;
  approve_or_reject_access_request(
    &state.pg_pool,
    state.workspace_access_control.clone(),
    state.mailer.clone(),
    &state.config.appflowy_web_url,
    access_request_id,
    uid,
    is_approved,
  )
  .await?;
  Ok(Json(AppResponse::Ok()))
}
