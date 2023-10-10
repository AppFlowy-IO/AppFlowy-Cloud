use crate::access_token::WebAccessToken;
use crate::error::RenderError;
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

pub async fn home_handler(_access_token: WebAccessToken) -> Result<Html<String>, RenderError> {
  let s = templates::Home {}.render()?;
  Ok(Html(s))
}

pub async fn admin_handler(_access_token: WebAccessToken) -> Result<Html<String>, RenderError> {
  let s = templates::Admin {}.render()?;
  Ok(Html(s))
}

pub async fn admin_users_handler(
  State(state): State<AppState>,
  access_token: WebAccessToken,
) -> Result<Html<String>, RenderError> {
  let users = state
    .gotrue_client
    .admin_list_user(&access_token.0)
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
  access_token: WebAccessToken,
  Path(user_id): Path<String>,
) -> Result<Html<String>, RenderError> {
  println!("---------- user_id: {}", user_id);
  // http://localhost:3000/admin/users/3c093675-c4e0-4329-bd92-c1e8345afdd2
  let users = state
    .gotrue_client
    .admin_user_details(&access_token.0, &user_id)
    .await
    .unwrap(); // TODO: handle error
  let s = templates::UserDetails { user: &users }.render()?;
  Ok(Html(s))
}

pub async fn login_handler() -> Result<Html<String>, RenderError> {
  let s = templates::Login {}.render()?;
  Ok(Html(s))
}
