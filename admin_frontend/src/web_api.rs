use std::borrow::Cow;

use axum::response::Result;
use axum::{extract::State, routing::post, Json, Router};
use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;

use crate::error::WebApiError;
use crate::{models::LoginRequest, AppState};

pub fn router() -> Router<AppState> {
  Router::new()
      // TODO
    .route("/login", post(login_handler))
}

// TODO: Support OAuth2 login
// login and set the cookie
pub async fn login_handler(
  cookie_jar: CookieJar,
  State(state): State<AppState>,
  Json(param): Json<LoginRequest>,
) -> Result<CookieJar, WebApiError> {
  let token = state
    .gotrue_client
    .token(&gotrue::grant::Grant::Password(
      gotrue::grant::PasswordGrant {
        email: param.email.to_owned(),
        password: param.password.to_owned(),
      },
    ))
    .await?;

  Ok(set_access_token_cookie(
    cookie_jar,
    token.access_token.into(),
  ))
}

fn set_access_token_cookie(jar: CookieJar, token: Cow<'static, str>) -> CookieJar {
  jar.add(Cookie::new("access_token", token))
}
