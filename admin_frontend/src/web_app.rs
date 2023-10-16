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

pub async fn login_handler() -> Result<Html<String>, RenderError> {
  let s = templates::Login {}.render()?;
  Ok(Html(s))
}

pub async fn home_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, RenderError> {
  let user_email = state
    .gotrue_client
    .user_info(&session.access_token)
    .await
    .map(|user_info| user_info.email)
    .unwrap_or_else(|err| {
      tracing::error!("Error getting user info: {:?}", err);
      "".to_owned()
    });

  let s = templates::Home { email: &user_email }.render()?;
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
        tracing::error!("Error getting user list: {:?}", err);
        vec![]
      },
      |r| r.users,
    )
    .into_iter()
    .filter(|user| user.deleted_at.is_none())
    .collect::<Vec<_>>();

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
