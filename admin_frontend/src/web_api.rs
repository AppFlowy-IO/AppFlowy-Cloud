use crate::error::WebApiError;
use crate::models::PutUserRequest;
use crate::response::WebApiResponse;
use crate::session::{self, UserSession};
use crate::{models::LoginRequest, AppState};
use axum::extract::Path;
use axum::http::status;
use axum::response::Result;
use axum::Json;
use axum::{extract::State, routing::post, Router};
use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use gotrue::params::{
  AdminDeleteUserParams, AdminUserParams, GenerateLinkParams, GenerateLinkResponse,
};
use gotrue_entity::User;

pub fn router() -> Router<AppState> {
  Router::new()
      // TODO
    .route("/login", post(login_handler))
    .route("/logout", post(logout_handler))
    .route("/user/:param", post(post_user_handler).delete(delete_user_handler).put(put_user_handler))
    .route("/user/:email/generate-link", post(post_user_generate_link_handler))
}

pub async fn put_user_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(user_uuid): Path<String>,
  Json(param): Json<PutUserRequest>,
) -> Result<WebApiResponse<User>, WebApiError<'static>> {
  let res = state
    .gotrue_client
    .admin_put_user(
      &session.access_token,
      &user_uuid,
      &AdminUserParams {
        email: param.email.to_owned(),
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

pub async fn delete_user_handler(
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

pub async fn post_user_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(email): Path<String>,
) -> Result<WebApiResponse<User>, WebApiError<'static>> {
  let add_user_params = AdminUserParams {
    email,
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
