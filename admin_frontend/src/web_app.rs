use crate::access_token::WebAccessToken;
use crate::error::RenderError;
use askama::Template;
use axum::extract::State;
use axum::response::Result;
use axum::{response::Html, routing::get, Router};

use crate::{templates, AppState};

pub fn router() -> Router<AppState> {
  Router::new()
    .route("/", get(home_handler))
    .route("/home", get(home_handler))
    .route("/login", get(login_handler))
    .route("/admin", get(admin_handler))
    .route("/admin/users", get(admin_users_handler))
}

pub async fn home_handler(access_token: WebAccessToken) -> Result<Html<String>, RenderError> {
  let s = templates::Home {}.render()?;
  Ok(Html(s))
}

pub async fn admin_handler(access_token: WebAccessToken) -> Result<Html<String>, RenderError> {
  let s = templates::Admin {}.render()?;
  Ok(Html(s))
}

pub async fn admin_users_handler(
  State(state): State<AppState>,
  access_token: WebAccessToken,
) -> Result<Html<String>, RenderError> {
  let users = state.gotrue_client.admin_list_user(&access_token.0).await;
  todo!()
}

pub async fn login_handler() -> Result<Html<String>, RenderError> {
  let s = templates::Login {}.render()?;
  Ok(Html(s))
}
