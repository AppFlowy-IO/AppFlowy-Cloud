use crate::biz;
use crate::component::auth::{
  change_password, logged_user_from_request, login, logout, register, ChangePasswordRequest,
  InternalServerError, RegisterRequest,
};
use crate::component::auth::{InputParamsError, LoginRequest};
use crate::component::token_state::SessionToken;
use crate::domain::{UserEmail, UserName, UserPassword};
use crate::state::AppState;
use gotrue_entity::{AccessTokenResponse, OAuthProvider, OAuthURL, User};
use shared_entity::data::{AppResponse, JsonAppResponse};
use shared_entity::dto::{
  SignInParams, SignInPasswordResponse, SignInTokenResponse, UserUpdateParams,
};
use shared_entity::error::AppError;
use shared_entity::error_code::ErrorCode;
use storage_entity::AFUserProfileView;

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
    .service(web::resource("/sign_in/token/{access_token}").route(web::get().to(sign_in_token_handler)))
    .service(web::resource("/sign_out").route(web::post().to(sign_out_handler)))
    .service(web::resource("/update").route(web::post().to(update_handler)))
    .service(web::resource("/oauth/{provider}").route(web::get().to(oauth_handler)))
    .service(web::resource("/refresh/{refresh_token}").route(web::get().to(refresh_handler)))
    .service(web::resource("/profile").route(web::get().to(profile_handler)))

    // native
    .service(web::resource("/login").route(web::post().to(login_handler)))
    .service(web::resource("/logout").route(web::get().to(logout_handler)))
    .service(web::resource("/register").route(web::post().to(register_handler)))
    .service(web::resource("/password").route(web::post().to(change_password_handler)))
}

async fn refresh_handler(
  path: web::Path<String>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AccessTokenResponse>> {
  let refresh_token = path.into_inner();
  let oauth_url = biz::user::refresh(&state.gotrue_client, refresh_token).await?;
  Ok(AppResponse::Ok().with_data(oauth_url).into())
}

async fn sign_in_token_handler(
  path: web::Path<String>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<SignInTokenResponse>> {
  let access_token = path.into_inner();
  let (user, is_new) =
    biz::user::sign_in_token(&state.pg_pool, &state.gotrue_client, &access_token).await?;
  let resp = SignInTokenResponse { user, is_new };
  Ok(AppResponse::Ok().with_data(resp).into())
}

async fn oauth_handler(
  path: web::Path<String>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<OAuthURL>> {
  let provider = path.into_inner();
  let provider =
    OAuthProvider::from(&provider).ok_or::<AppError>(ErrorCode::InvalidOAuthProvider.into())?;
  let oauth_url = biz::user::oauth(&state.gotrue_client, provider).await?;
  Ok(AppResponse::Ok().with_data(oauth_url).into())
}

async fn profile_handler(
  uuid: UserUuid,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AFUserProfileView>> {
  let profile = biz::user::get_profile(&state.pg_pool, &uuid).await?;
  Ok(AppResponse::Ok().with_data(profile).into())
}

async fn update_handler(
  auth: Authorization,
  req: Json<UserUpdateParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<User>> {
  let req = req.into_inner();
  let user = biz::user::update(
    &state.pg_pool,
    &state.gotrue_client,
    &auth.token,
    &req.email,
    &req.password,
    req.name.as_deref(),
  )
  .await?;
  Ok(AppResponse::Ok().with_data(user).into())
}

async fn sign_out_handler(
  auth: Authorization,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  state
    .gotrue_client
    .logout(&auth.token)
    .await
    .map_err(InternalServerError::new)?;

  Ok(AppResponse::Ok().into())
}

async fn sign_in_password_handler(
  req: Json<SignInParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<SignInPasswordResponse>> {
  let req = req.into_inner();
  let (token, new) = biz::user::sign_in_password(
    &state.pg_pool,
    &state.gotrue_client,
    req.email,
    req.password,
  )
  .await?;
  let resp = SignInPasswordResponse {
    access_token_resp: token,
    is_new: new,
  };
  Ok(AppResponse::Ok().with_data(resp).into())
}

async fn sign_up_handler(
  req: Json<SignInParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  let req = req.into_inner();
  biz::user::sign_up(&state.gotrue_client, req.email, req.password).await?;

  Ok(AppResponse::Ok().into())
}

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
