use crate::error::WebAppError;
use crate::ext::{get_user_workspace_count, get_user_workspace_limit};
use crate::session::UserSession;
use askama::Template;
use axum::extract::{Path, State};
use axum::response::Result;
use axum::{response::Html, routing::get, Router};
use gotrue_entity::dto::User;
use human_bytes::human_bytes;

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
    .route("/user/user-usage", get(user_usage_handler))
    .route("/user/workspace-usage", get(workspace_usage_handler))

    // Admin actions
    .route("/admin/navigate", get(admin_navigate_handler))
    .route("/admin/users", get(admin_users_handler))
    .route("/admin/users/:user_id", get(admin_user_details_handler))
    .route("/admin/users/create", get(admin_users_create_handler))
    // SSO
    .route("/admin/sso", get(admin_sso_handler))
    .route("/admin/sso/create", get(admin_sso_create_handler))
    .route("/admin/sso/:sso_provider_id", get(admin_sso_detail_handler))
}

pub async fn admin_sso_detail_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(sso_provider_id): Path<String>,
) -> Result<Html<String>, WebAppError> {
  let sso_provider = state
    .gotrue_client
    .admin_get_sso_provider(&session.token.access_token, &sso_provider_id)
    .await?;

  let mapping_json =
    serde_json::to_string_pretty(&sso_provider.saml.attribute_mapping).unwrap_or("".to_owned());

  render_template(templates::SsoDetail {
    sso_provider,
    mapping_json,
  })
}

pub async fn admin_sso_create_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::SsoCreate)
}

pub async fn admin_sso_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let sso_providers = state
    .gotrue_client
    .admin_list_sso_providers(&session.token.access_token)
    .await?
    .items
    .unwrap_or_default();

  render_template(templates::SsoList { sso_providers })
}

pub async fn user_navigate_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::Navigate)
}

pub async fn admin_navigate_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::AdminNavigate)
}

pub async fn user_invite_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::Invite)
}

pub async fn user_usage_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let workspace_count =
    get_user_workspace_count(&session.token.access_token, &state.appflowy_cloud_url)
      .await
      .unwrap_or_else(|err| {
        tracing::error!("Error getting user workspace count: {:?}", err);
        0
      });

  let workspace_limit = get_user_workspace_limit(
    &session.token.access_token,
    &state.appflowy_cloud_gateway_url,
  )
  .await
  .unwrap_or_else(|err| {
    tracing::error!("Error getting user workspace limit: {:?}", err);
    0
  });

  render_template(templates::UserUsage {
    workspace_count,
    workspace_limit,
  })
}

pub async fn workspace_usage_handler(session: UserSession) -> Result<Html<String>, WebAppError> {
  render_template(templates::WorkspaceUsage {
    name: "test",
    member_count: 6,
    member_limit: 7,
    total_doc_size: &human_bytes(987654),
    total_blob_size: &human_bytes(9876543),
    total_blob_limit: &human_bytes(98765432),
  })
}

pub async fn admin_users_create_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::CreateUser)
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

pub async fn login_handler(State(state): State<AppState>) -> Result<Html<String>, WebAppError> {
  let external = state.gotrue_client.settings().await?.external;
  let oauth_providers = external.oauth_providers();
  render_template(templates::Login { oauth_providers })
}

pub async fn user_change_password_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::ChangePassword)
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
    .admin_list_user(&session.token.access_token, None)
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
