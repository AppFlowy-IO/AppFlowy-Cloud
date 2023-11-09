use crate::error::WebAppError;
use crate::session::UserSession;
use askama::Template;
use axum::extract::{Path, State};
use axum::response::Result;
use axum::{response::Html, routing::get, Router};
use gotrue_entity::dto::User;

use crate::{templates, AppState};

pub fn router(state: AppState) -> Router<AppState> {
  Router::new()
    .nest_service("/", page_router().with_state(state.clone()))
    .nest_service("/components", component_router().with_state(state))
}

pub fn page_router() -> Router<AppState> {
  Router::new()
    .route("/", get(home_handler))
    .route("/login", get(login_handler))
    .route("/home", get(home_handler))
    .route("/admin/home", get(admin_home_handler))
}

pub fn component_router() -> Router<AppState> {
  Router::new()
    // User actions
    .route("/user/navigate", get(user_navigate_handler))
    .route("/user/user", get(user_user_handler))
    .route("/user/change_password", get(user_change_password_handler))
    .route("/user/invite", get(user_invite_handler))

    // Admin actions
    .route("/admin/navigate", get(admin_navigate_handler))
    .route("/admin/users", get(admin_users_handler))
    .route("/admin/users/:user_id", get(admin_user_details_handler))
    .route("/admin/users/create", get(admin_users_create_handler))
}

pub async fn user_navigate_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::Navigate {})
}

pub async fn admin_navigate_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::AdminNavigate {})
}

pub async fn user_invite_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::Invite {})
}

pub async fn admin_users_create_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::CreateUser {})
}

pub async fn user_user_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let user = state
    .gotrue_client
    .user_info(&session.token.access_token)
    .await?;
  render_template(templates::UserDetails { user: &user })
}

pub async fn login_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::Login {})
}

pub async fn user_change_password_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::ChangePassword {})
}

pub async fn home_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let user = state
    .gotrue_client
    .user_info(&session.token.access_token)
    .await?;
  render_template(templates::Home {
    user: &user,
    is_admin: is_admin(&user),
  })
}

pub async fn admin_home_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let user = state
    .gotrue_client
    .user_info(&session.token.access_token)
    .await?;
  render_template(templates::AdminHome { user: &user })
}

pub async fn admin_users_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let users = state
    .gotrue_client
    .admin_list_user(&session.token.access_token)
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

  render_template(templates::AdminUsers { users: &users })
}

pub async fn admin_user_details_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(user_id): Path<String>,
) -> Result<Html<String>, WebAppError> {
  let user = state
    .gotrue_client
    .admin_user_details(&session.token.access_token, &user_id)
    .await
    .unwrap(); // TODO: handle error

  render_template(templates::AdminUserDetails { user: &user })
}

fn render_template<T>(x: T) -> Result<Html<String>, WebAppError>
where
  T: Template,
{
  let s = x.render()?;
  Ok(Html(s))
}

fn is_admin(user: &User) -> bool {
  user.role == "supabase_admin"
}
