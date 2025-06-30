use crate::biz::authentication::jwt::{Authorization, UserUuid};
use crate::biz::user::user_delete::delete_user;
use crate::biz::user::user_info::{get_profile, get_user_workspace_info, update_user};
use crate::biz::user::user_verify::verify_token;
use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::Result;
use actix_web::{web, Scope};
use database_entity::dto::{AFUserProfile, AFUserWorkspaceInfo};
use shared_entity::dto::auth_dto::{DeleteUserQuery, SignInTokenResponse, UpdateUserParams};
use shared_entity::response::AppResponseError;
use shared_entity::response::{AppResponse, JsonAppResponse};

pub fn user_scope() -> Scope {
  web::scope("/api/user")
    .service(web::resource("/verify/{access_token}").route(web::get().to(verify_user_handler)))
    .service(web::resource("/update").route(web::post().to(update_user_handler)))
    .service(web::resource("/profile").route(web::get().to(get_user_profile_handler)))
    .service(web::resource("/workspace").route(web::get().to(get_user_workspace_info_handler)))
    .service(web::resource("").route(web::delete().to(delete_user_handler)))
}

#[tracing::instrument(skip(state, path), err)]
async fn verify_user_handler(
  path: web::Path<String>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<SignInTokenResponse>> {
  let access_token = path.into_inner();
  let is_new = verify_token(&access_token, state.as_ref())
    .await
    .map_err(AppResponseError::from)?;
  let resp = SignInTokenResponse { is_new };
  Ok(AppResponse::Ok().with_data(resp).into())
}

#[tracing::instrument(skip(state), err)]
async fn get_user_profile_handler(
  uuid: UserUuid,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AFUserProfile>> {
  let profile = get_profile(&state.pg_pool, &uuid)
    .await
    .map_err(AppResponseError::from)?;
  Ok(AppResponse::Ok().with_data(profile).into())
}

#[tracing::instrument(skip(state), err)]
async fn get_user_workspace_info_handler(
  uuid: UserUuid,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AFUserWorkspaceInfo>> {
  let info = get_user_workspace_info(&state.pg_pool, &uuid).await?;
  Ok(AppResponse::Ok().with_data(info).into())
}

#[tracing::instrument(skip(state, auth, payload), err)]
async fn update_user_handler(
  auth: Authorization,
  payload: Json<UpdateUserParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  let params = payload.into_inner();
  update_user(&state.pg_pool, auth.uuid()?, params).await?;
  Ok(AppResponse::Ok().into())
}

#[tracing::instrument(skip(state), err)]
async fn delete_user_handler(
  auth: Authorization,
  state: Data<AppState>,
  query: web::Query<DeleteUserQuery>,
) -> Result<JsonAppResponse<()>, actix_web::Error> {
  let user_uuid = auth.uuid()?;
  let DeleteUserQuery {
    provider_access_token,
    provider_refresh_token,
  } = query.into_inner();
  delete_user(
    &state.pg_pool,
    &state.redis_connection_manager,
    &state.bucket_storage,
    &state.gotrue_client,
    &state.gotrue_admin,
    &state.config.apple_oauth,
    auth,
    user_uuid,
    provider_access_token,
    provider_refresh_token,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}
