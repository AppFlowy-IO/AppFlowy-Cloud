use crate::error::WebApiError;
use crate::models::{ChangePasswordRequest, PutUserRequest, WebAdminCreateUserRequest};
use crate::response::WebApiResponse;
use crate::session::{self, UserSession};
use crate::{models::LoginRequest, AppState};
use axum::extract::Path;
use axum::http::{status, HeaderMap, HeaderValue};
use axum::response::Result;
use axum::routing::delete;
use axum::Json;
use axum::{extract::State, routing::post, Router};
use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use gotrue::params::{
  AdminDeleteUserParams, AdminUserParams, GenerateLinkParams, GenerateLinkResponse,
};
use gotrue_entity::dto::{UpdateGotrueUserParams, User};

pub fn router() -> Router<AppState> {
  Router::new()
    .route("/login", post(login_handler))
    .route("/login_refresh/:refresh_token", post(login_refresh_handler))
    .route("/logout", post(logout_handler))
    .route("/admin/user", post(admin_add_user_handler))
    .route(
      "/admin/user/:param",
      delete(admin_delete_user_handler).put(admin_update_user_handler),
    )
    .route(
      "/admin/user/:email/generate-link",
      post(post_user_generate_link_handler),
    )
    .route("/change_password", post(change_password_handler))
    .route("/oauth_login/:provider", post(post_oauth_login_handler))
}

pub async fn change_password_handler(
  State(state): State<AppState>,
  session: UserSession,
  Json(param): Json<ChangePasswordRequest>,
) -> Result<WebApiResponse<User>, WebApiError<'static>> {
  let res = state
    .gotrue_client
    .update_user(
      &session.access_token,
      &UpdateGotrueUserParams {
        password: Some(param.new_password),
        ..Default::default()
      },
    )
    .await?;
  Ok(res.into())
}

static DEFAULT_HOST: HeaderValue = HeaderValue::from_static("localhost");
static DEFAULT_SCHEME: HeaderValue = HeaderValue::from_static("http");
pub async fn post_oauth_login_handler(
  header_map: HeaderMap,
  Path(provider): Path<String>,
) -> Result<WebApiResponse<String>, WebApiError<'static>> {
  let host = header_map
    .get("host")
    .unwrap_or(&DEFAULT_HOST)
    .to_str()
    .unwrap();
  let scheme = header_map
    .get("x-scheme")
    .unwrap_or(&DEFAULT_SCHEME)
    .to_str()
    .unwrap();
  let base_url = format!("{}://{}", scheme, host);
  let redirect_uri = format!("{}/web/oauth_login_redirect", base_url);

  let oauth_url = format!(
    "{}/authorize?provider={}&redirect_uri={}",
    base_url, &provider, redirect_uri
  );
  Ok(oauth_url.into())
}

pub async fn admin_update_user_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(user_uuid): Path<String>,
  Json(param): Json<PutUserRequest>,
) -> Result<WebApiResponse<User>, WebApiError<'static>> {
  let res = state
    .gotrue_client
    .admin_update_user(
      &session.access_token,
      &user_uuid,
      &AdminUserParams {
        password: Some(param.password.to_owned()),
        email_confirm: true,
        ..Default::default()
      },
    )
    .await?;
  Ok(res.into())
}

pub async fn post_user_generate_link_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(email): Path<String>,
) -> Result<WebApiResponse<GenerateLinkResponse>, WebApiError<'static>> {
  let res = state
    .gotrue_client
    .generate_link(
      &session.access_token,
      &GenerateLinkParams {
        email,
        ..Default::default()
      },
    )
    .await?;
  Ok(res.into())
}

pub async fn admin_delete_user_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(user_uuid): Path<String>,
) -> Result<WebApiResponse<()>, WebApiError<'static>> {
  state
    .gotrue_client
    .admin_delete_user(
      &session.access_token,
      &user_uuid,
      &AdminDeleteUserParams {
        should_soft_delete: true,
      },
    )
    .await?;
  Ok(().into())
}

pub async fn admin_add_user_handler(
  State(state): State<AppState>,
  session: UserSession,
  Json(param): Json<WebAdminCreateUserRequest>,
) -> Result<WebApiResponse<User>, WebApiError<'static>> {
  let add_user_params = AdminUserParams {
    email: param.email,
    password: Some(param.password),
    email_confirm: !param.require_email_verification,
    ..Default::default()
  };
  let user = state
    .gotrue_client
    .admin_add_user(&session.access_token, &add_user_params)
    .await?;
  Ok(user.into())
}

pub async fn login_refresh_handler(
  State(state): State<AppState>,
  jar: CookieJar,
  Path(refresh_token): Path<String>,
) -> Result<CookieJar, WebApiError<'static>> {
  let token = state
    .gotrue_client
    .token(&gotrue::grant::Grant::RefreshToken(
      gotrue::grant::RefreshTokenGrant { refresh_token },
    ))
    .await?;

  let new_session_id = uuid::Uuid::new_v4();
  let new_session = session::UserSession::new(
    new_session_id.to_string(),
    token.access_token.to_string(),
    token.refresh_token.to_owned(),
  );
  state.session_store.put_user_session(&new_session).await?;

  let mut cookie = Cookie::new("session_id", new_session_id.to_string());
  cookie.set_path("/");

  Ok(jar.add(cookie))
}

// TODO: Support OAuth2 login
// login and set the cookie
pub async fn login_handler(
  State(state): State<AppState>,
  jar: CookieJar,
  Json(param): Json<LoginRequest>,
) -> Result<CookieJar, WebApiError<'static>> {
  let token = state
    .gotrue_client
    .token(&gotrue::grant::Grant::Password(
      gotrue::grant::PasswordGrant {
        email: param.email.to_owned(),
        password: param.password.to_owned(),
      },
    ))
    .await?;

  let new_session_id = uuid::Uuid::new_v4();
  let new_session = session::UserSession::new(
    new_session_id.to_string(),
    token.access_token.to_string(),
    token.refresh_token.to_owned(),
  );
  state.session_store.put_user_session(&new_session).await?;

  let mut cookie = Cookie::new("session_id", new_session_id.to_string());
  cookie.set_path("/");

  Ok(jar.add(cookie))
}

pub async fn logout_handler(
  State(state): State<AppState>,
  jar: CookieJar,
) -> Result<CookieJar, WebApiError<'static>> {
  let session_id = jar
    .get("session_id")
    .ok_or(WebApiError::new(
      status::StatusCode::BAD_REQUEST,
      "no session_id cookie",
    ))?
    .value();

  state.session_store.del_user_session(session_id).await?;
  Ok(jar.remove(Cookie::named("session_id")))
}
