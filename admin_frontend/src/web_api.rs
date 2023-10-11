use crate::error::WebApiError;
use crate::models::AddUserRequest;
use crate::response::WebApiResponse;
use crate::session::{self, UserSession};
use crate::{models::LoginRequest, AppState};
use axum::http::status;
use axum::response::Result;
use axum::Json;
use axum::{extract::State, routing::post, Router};
use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use gotrue::params::AdminUserParams;
use gotrue_entity::User;

pub fn router() -> Router<AppState> {
  Router::new()
      // TODO
    .route("/login", post(login_handler))
    .route("/logout", post(logout_handler))
    .route("/add_user", post(add_user_handler))
}

pub async fn add_user_handler(
  State(state): State<AppState>,
  session: UserSession,
  Json(param): Json<AddUserRequest>,
) -> Result<WebApiResponse<User>, WebApiError<'static>> {
  let add_user_params = AdminUserParams {
    email: param.email.to_owned(),
    ..Default::default()
  };
  let user = state
    .gotrue_client
    .admin_add_user(&session.access_token, &add_user_params)
    .await?;
  Ok(user.into())
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
  state.session_store.put_user_session(new_session).await?;

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
