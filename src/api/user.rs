use crate::biz;
use crate::component::auth::{
  change_password, logged_user_from_request, login, logout, register, ChangePasswordRequest,
  InternalServerError, RegisterRequest,
};
use crate::component::auth::{InputParamsError, LoginRequest};
use crate::component::token_state::SessionToken;
use crate::domain::{UserEmail, UserName, UserPassword};
use crate::state::State;
use gotrue::models::{AccessTokenResponse, User};
use shared_entity::data::{AppResponse, JsonAppResponse};
use storage::entities::{AfUserProfileView, AfWorkspaces};

use crate::component::auth::jwt::{Authorization, UserUuid};
use actix_web::web::{Data, Json};
use actix_web::HttpRequest;
use actix_web::Result;
use actix_web::{web, HttpResponse, Scope};

pub fn user_scope() -> Scope {
  web::scope("/api/user")

    // auth server integration
    .service(web::resource("/sign_up").route(web::post().to(sign_up_handler)))
    .service(web::resource("/sign_in/password").route(web::post().to(sign_in_password_handler)))
    .service(web::resource("/sign_out").route(web::post().to(sign_out_handler)))
    .service(web::resource("/update").route(web::post().to(update_handler)))

    .service(web::resource("/workspaces").route(web::get().to(workspaces_handler)))
    .service(web::resource("/profile").route(web::get().to(profile_handler)))

    // native
    .service(web::resource("/login").route(web::post().to(login_handler)))
    .service(web::resource("/logout").route(web::get().to(logout_handler)))
    .service(web::resource("/register").route(web::post().to(register_handler)))
    .service(web::resource("/password").route(web::post().to(change_password_handler)))
}

async fn profile_handler(
  uuid: UserUuid,
  state: Data<State>,
) -> Result<JsonAppResponse<AfUserProfileView>> {
  let profile = biz::user::user_profile(&state.pg_pool, &uuid.0).await?;
  Ok(AppResponse::Ok().with_data(profile).into())
}

async fn workspaces_handler(
  uuid: UserUuid,
  state: Data<State>,
) -> Result<JsonAppResponse<AfWorkspaces>> {
  let workspaces = biz::user::user_workspaces(&state.pg_pool, &uuid.0).await?;
  Ok(AppResponse::Ok().with_data(workspaces).into())
}

async fn update_handler(
  auth: Authorization,
  req: Json<LoginRequest>,
  state: Data<State>,
) -> Result<JsonAppResponse<User>> {
  let req = req.into_inner();
  let user =
    biz::user::update(&state.gotrue_client, &auth.token, &req.email, &req.password).await?;
  Ok(AppResponse::Ok().with_data(user).into())
}

async fn sign_out_handler(auth: Authorization, state: Data<State>) -> Result<JsonAppResponse<()>> {
  state
    .gotrue_client
    .logout(&auth.token)
    .await
    .map_err(InternalServerError::new)?;
  Ok(AppResponse::Ok().into())
}

async fn sign_in_password_handler(
  req: Json<LoginRequest>,
  state: Data<State>,
) -> Result<JsonAppResponse<AccessTokenResponse>> {
  let req = req.into_inner();
  let token = biz::user::sign_in(
    &state.pg_pool,
    &state.gotrue_client,
    req.email,
    req.password,
  )
  .await?;
  Ok(AppResponse::Ok().with_data(token).into())
}

async fn sign_up_handler(
  req: Json<LoginRequest>,
  state: Data<State>,
) -> Result<JsonAppResponse<()>> {
  biz::user::sign_up(
    &state.pg_pool,
    &state.gotrue_client,
    &req.email,
    &req.password,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

async fn login_handler(
  req: Json<LoginRequest>,
  state: Data<State>,
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

async fn logout_handler(req: HttpRequest, state: Data<State>) -> Result<HttpResponse> {
  let logged_user = logged_user_from_request(&req, &state.config.application.server_key)?;
  logout(logged_user, state.user.clone()).await;
  Ok(HttpResponse::Ok().finish())
}

#[tracing::instrument(level = "debug", skip(state))]
async fn register_handler(req: Json<RegisterRequest>, state: Data<State>) -> Result<HttpResponse> {
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

async fn change_password_handler(
  req: HttpRequest,
  payload: Json<ChangePasswordRequest>,
  // session: SessionToken,
  state: Data<State>,
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
