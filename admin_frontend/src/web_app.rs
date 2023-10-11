use crate::error::RenderError;
use crate::session::UserSession;
use askama::Template;
use axum::extract::{Path, State};
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
    .route("/admin/users/:user_id", get(admin_user_details_handler))
}

pub async fn home_handler(_: UserSession) -> Result<Html<String>, RenderError> {
  let s = templates::Home {}.render()?;
  Ok(Html(s))
}

pub async fn admin_handler(_: UserSession) -> Result<Html<String>, RenderError> {
  let s = templates::Admin {}.render()?;
  Ok(Html(s))
}

pub async fn admin_users_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, RenderError> {
  let users = state
    .gotrue_client
    .admin_list_user(&session.access_token)
    .await
    .map_or_else(
      |err| {
        // Log the error and return an empty vector.
        println!("Failed to fetch users: {:?}", err);
        vec![]
      },
      |r| r.users,
    );
  let s = templates::Users { users: &users }.render()?;
  Ok(Html(s))
}

pub async fn admin_user_details_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(user_id): Path<String>,
) -> Result<Html<String>, RenderError> {
  let users = state
    .gotrue_client
    .admin_user_details(&session.access_token, &user_id)
    .await
    .unwrap(); // TODO: handle error
  let s = templates::UserDetails { user: &users }.render()?;
  Ok(Html(s))
}

pub async fn login_handler() -> Result<Html<String>, RenderError> {
  let s = templates::Login {}.render()?;
  Ok(Html(s))
}
