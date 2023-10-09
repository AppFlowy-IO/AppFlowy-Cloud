use crate::error::RenderError;
use askama::Template;
use axum::response::Result;
use axum::{response::Html, routing::get, Router};
use axum_extra::extract::cookie::CookieJar;

use crate::templates;

pub fn router() -> Router {
  Router::new()
    .route("/", get(home_handler))
    .route("/home", get(home_handler))
    .route("/login", get(login_handler))
}

pub async fn home_handler(cookies: CookieJar) -> Result<Html<String>, RenderError> {
  let access_token = cookies.get("access_token");
  match access_token {
    Some(access_token) => Ok(Html(access_token.to_string())), // TODO: render home page
    None => login_handler().await,
  }
}

pub async fn login_handler() -> Result<Html<String>, RenderError> {
  let s = templates::Login {}.render()?;
  Ok(Html(s))
}
