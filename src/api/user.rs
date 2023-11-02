use crate::biz;
use crate::component::auth::{
  change_password, logged_user_from_request, login, logout, register, ChangePasswordRequest,
  RegisterRequest,
};

use crate::component::auth::{InputParamsError, LoginRequest};

use crate::component::token_state::SessionToken;
use crate::domain::{UserEmail, UserName, UserPassword};
use crate::state::AppState;
use shared_entity::dto::auth_dto::{SignInTokenResponse, UpdateUserParams};
use shared_entity::response::{AppResponse, JsonAppResponse};

use crate::component::auth::jwt::{Authorization, UserUuid};
use actix_web::web::{Data, Json};
use actix_web::HttpRequest;
use actix_web::Result;
use actix_web::{web, HttpResponse, Scope};
use database_entity::dto::{AFUserProfile, AFUserWorkspaceInfo};

use shared_entity::response::AppResponseError;
use tracing_actix_web::RequestId;

pub fn user_scope() -> Scope {
  web::scope("/api/user")
    // auth server integration
    .service(web::resource("/verify/{access_token}").route(web::get().to(verify_user_handler)))
    .service(web::resource("/update").route(web::post().to(update_user_handler)))
    .service(web::resource("/profile").route(web::get().to(get_user_profile_handler)))
    .service(web::resource("/workspace").route(web::get().to(get_user_workspace_info_handler)))

    // deprecated
    .service(web::resource("/login").route(web::post().to(login_handler)))
    .service(web::resource("/logout").route(web::get().to(logout_handler)))
    .service(web::resource("/register").route(web::post().to(register_handler)))
    .service(web::resource("/password").route(web::post().to(change_password_handler)))
}

#[tracing::instrument(skip(state, path), err)]
async fn verify_user_handler(
  path: web::Path<String>,
  state: Data<AppState>,
  request_id: RequestId,
) -> Result<JsonAppResponse<SignInTokenResponse>> {
  let access_token = path.into_inner();
  let is_new = biz::user::verify_token(
    &state.pg_pool,
    &state.id_gen,
    &state.gotrue_client,
    &access_token,
  )
  .await
  .map_err(AppResponseError::from)?;
  let resp = SignInTokenResponse { is_new };
  Ok(AppResponse::Ok().with_data(resp).into())
}

#[tracing::instrument(skip(state), err)]
async fn get_user_profile_handler(
  uuid: UserUuid,
  state: Data<AppState>,
  request_id: RequestId,
) -> Result<JsonAppResponse<AFUserProfile>> {
  let profile = biz::user::get_profile(&state.pg_pool, &uuid)
    .await
    .map_err(AppResponseError::from)?;
  Ok(AppResponse::Ok().with_data(profile).into())
}

#[tracing::instrument(skip(state), err)]
async fn get_user_workspace_info_handler(
  uuid: UserUuid,
  state: Data<AppState>,
  request_id: RequestId,
) -> Result<JsonAppResponse<AFUserWorkspaceInfo>> {
  let info = biz::user::get_user_workspace_info(&state.pg_pool, &uuid).await?;
  Ok(AppResponse::Ok().with_data(info).into())
}

#[tracing::instrument(skip(state, auth, payload), err)]
async fn update_user_handler(
  auth: Authorization,
  payload: Json<UpdateUserParams>,
  state: Data<AppState>,
  request_id: RequestId,
) -> Result<JsonAppResponse<()>> {
  let params = payload.into_inner();
  biz::user::update_user(&state.pg_pool, auth.uuid()?, params).await?;
  Ok(AppResponse::Ok().into())
}

#[tracing::instrument(skip_all)]
async fn login_handler(
  req: Json<LoginRequest>,
  state: Data<AppState>,
  session: SessionToken,
) -> Result<HttpResponse> {
  let req = req.into_inner();
  let email = UserEmail::parse(req.email)
    .map_err(InputParamsError::InvalidEmail)?
    .0;
  let password = UserPassword::parse(req.password)
    .map_err(InputParamsError::InvalidPassword)?
    .0;
  let (resp, token) = login(email, password, &state).await?;

  // Renews the session key, assigning existing session state to new key.
  session.renew();
  if let Err(err) = session.insert_token(token) {
    // It needs to navigate to login page in web application
    tracing::error!("Insert session failed: {:?}", err);
  }

  Ok(HttpResponse::Ok().json(resp))
}

#[tracing::instrument(level = "debug", skip(state))]
async fn logout_handler(req: HttpRequest, state: Data<AppState>) -> Result<HttpResponse> {
  let logged_user = logged_user_from_request(&req, &state.config.application.server_key)?;
  logout(logged_user, state.user.clone()).await;
  Ok(HttpResponse::Ok().finish())
}

#[tracing::instrument(level = "debug", skip(state))]
async fn register_handler(
  req: Json<RegisterRequest>,
  state: Data<AppState>,
) -> Result<HttpResponse> {
  let req = req.into_inner();
  let name = UserName::parse(req.name)
    .map_err(InputParamsError::InvalidName)?
    .0;
  let email = UserEmail::parse(req.email)
    .map_err(InputParamsError::InvalidEmail)?
    .0;
  let password = UserPassword::parse(req.password)
    .map_err(InputParamsError::InvalidPassword)?
    .0;

  let resp = register(name, email, password, &state).await?;
  Ok(HttpResponse::Ok().json(resp))
}

#[tracing::instrument(level = "debug", skip_all)]
async fn change_password_handler(
  req: HttpRequest,
  payload: Json<ChangePasswordRequest>,
  // session: SessionToken,
  state: Data<AppState>,
) -> Result<HttpResponse> {
  let logged_user = logged_user_from_request(&req, &state.config.application.server_key)?;
  let payload = payload.into_inner();
  if payload.new_password != payload.new_password_confirm {
    return Err(InputParamsError::PasswordNotMatch.into());
  }

  let new_password = UserPassword::parse(payload.new_password)
    .map_err(InputParamsError::InvalidPassword)?
    .0;

  change_password(
    state.pg_pool.clone(),
    logged_user.clone(),
    payload.current_password,
    new_password,
  )
  .await?;

  Ok(HttpResponse::Ok().finish())
}
