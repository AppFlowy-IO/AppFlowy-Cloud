use crate::biz;
use crate::component::auth::{
  change_password, logged_user_from_request, login, logout, register, ChangePasswordRequest,
  RegisterRequest,
};

use crate::component::auth::{InputParamsError, LoginRequest};

use crate::component::token_state::SessionToken;
use crate::domain::{UserEmail, UserName, UserPassword};
use crate::state::AppState;
use database_entity::AFUserProfileView;
use shared_entity::data::{AppResponse, JsonAppResponse};
use shared_entity::dto::{SignInTokenResponse, UpdateUsernameParams};

use crate::component::auth::jwt::{Authorization, UserUuid};
use actix_web::web::{Data, Json};
use actix_web::HttpRequest;
use actix_web::Result;
use actix_web::{web, HttpResponse, Scope};
use tracing_actix_web::RequestId;

pub fn user_scope() -> Scope {
  web::scope("/api/user")
    // auth server integration
    .service(web::resource("/verify/{access_token}").route(web::get().to(verify_handler)))
    .service(web::resource("/update").route(web::post().to(update_handler)))
    .service(web::resource("/profile").route(web::get().to(profile_handler)))

    // native
    .service(web::resource("/login").route(web::post().to(login_handler)))
    .service(web::resource("/logout").route(web::get().to(logout_handler)))
    .service(web::resource("/register").route(web::post().to(register_handler)))
    .service(web::resource("/password").route(web::post().to(change_password_handler)))
}

#[tracing::instrument(skip(state, path), err)]
async fn verify_handler(
  path: web::Path<String>,
  state: Data<AppState>,
  required_id: RequestId,
) -> Result<JsonAppResponse<SignInTokenResponse>> {
  let access_token = path.into_inner();
  let is_new = biz::user::token_verify(&state.pg_pool, &state.gotrue_client, &access_token).await?;
  let resp = SignInTokenResponse { is_new };
  Ok(AppResponse::Ok().with_data(resp).into())
}

#[tracing::instrument(skip(state), err)]
async fn profile_handler(
  uuid: UserUuid,
  state: Data<AppState>,
  required_id: RequestId,
) -> Result<JsonAppResponse<AFUserProfileView>> {
  let profile = biz::user::get_profile(&state.pg_pool, &uuid).await?;
  Ok(AppResponse::Ok().with_data(profile).into())
}

#[tracing::instrument(skip(state, auth, req), err)]
async fn update_handler(
  auth: Authorization,
  req: Json<UpdateUsernameParams>,
  state: Data<AppState>,
  required_id: RequestId,
) -> Result<JsonAppResponse<()>> {
  let params = req.into_inner();
  biz::user::update_user(&state.pg_pool, &auth.uuid()?, &params.new_name).await?;
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
