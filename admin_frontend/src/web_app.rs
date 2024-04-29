use crate::askama_entities::WorkspaceWithMembers;
use crate::error::WebAppError;
use crate::ext::api::{
  get_pending_workspace_invitations, get_user_owned_workspaces, get_user_profile,
  get_user_workspace_limit, get_user_workspace_usages, get_user_workspaces, get_workspace_members,
};
use crate::models::WebAppOAuthLoginRequest;
use crate::session::UserSession;
use askama::Template;
use axum::extract::{Path, Query, State};
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
    .route("/login-callback", get(login_callback_handler))
    .route("/login-callback-query", get(login_callback_query_handler))
    .route("/home", get(home_handler))
    .route("/admin/home", get(admin_home_handler))
}

pub fn component_router() -> Router<AppState> {
  Router::new()
    // User actions
    .route("/user/navigate", get(user_navigate_handler))
    .route("/user/user", get(user_user_handler))
    .route("/user/change-password", get(user_change_password_handler))
    .route("/user/invite", get(user_invite_handler))
    .route("/user/shared-workspaces", get(shared_workspaces_handler))
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

pub async fn login_callback_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::LoginCallback {})
}

pub async fn login_callback_query_handler(
  State(state): State<AppState>,
  Query(query): Query<WebAppOAuthLoginRequest>,
) -> Result<Html<String>, WebAppError> {
  println!("{:?}", query);

  // let token = state
  //   .gotrue_client
  //   .token(&gotrue::grant::Grant::RefreshToken(
  //     gotrue::grant::RefreshTokenGrant {
  //       refresh_token: query.refresh_token,
  //     },
  //   ))
  //   .await?;

  // // Do another round of refresh_token to consume and invalidate the old one
  // let token = state
  //   .gotrue_client
  //   .token(&gotrue::grant::Grant::RefreshToken(
  //     gotrue::grant::RefreshTokenGrant {
  //       refresh_token: token.refresh_token,
  //     },
  //   ))
  //   .await?;

  todo!()
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

pub async fn shared_workspaces_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let user_workspaces =
    get_user_workspaces(&session.token.access_token, &state.appflowy_cloud_url).await?;

  let profile = get_user_profile(
    session.token.access_token.as_str(),
    state.appflowy_cloud_url.as_str(),
  )
  .await?;

  let shared_workspaces = user_workspaces
    .into_iter()
    .filter(|workspace| workspace.owner_uid != profile.uid)
    .collect::<Vec<_>>();

  render_template(templates::SharedWorkspaces { shared_workspaces })
}

pub async fn user_invite_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let user_workspaces =
    get_user_workspaces(&session.token.access_token, &state.appflowy_cloud_url).await?;

  let profile = get_user_profile(
    session.token.access_token.as_str(),
    state.appflowy_cloud_url.as_str(),
  )
  .await?;

  let mut shared_workspaces = Vec::new();
  let mut owned_workspaces = Vec::with_capacity(user_workspaces.len());

  for workspace in user_workspaces {
    if workspace.owner_uid == profile.uid {
      let members = get_workspace_members(
        workspace.workspace_id.to_string().as_str(),
        session.token.access_token.as_str(),
        state.appflowy_cloud_url.as_str(),
      )
      .await?;
      owned_workspaces.push(WorkspaceWithMembers { workspace, members });
    } else {
      shared_workspaces.push(workspace);
    }
  }

  let pending_workspace_invitations = get_pending_workspace_invitations(
    session.token.access_token.as_str(),
    state.appflowy_cloud_url.as_str(),
  )
  .await?;

  render_template(templates::Invite {
    shared_workspaces,
    owned_workspaces,
    pending_workspace_invitations,
  })
}

pub async fn user_usage_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let workspace_count =
    get_user_owned_workspaces(&session.token.access_token, &state.appflowy_cloud_url)
      .await
      .map(|workspaces| workspaces.len())
      .unwrap_or_else(|err| {
        tracing::error!("Error getting user workspace count: {:?}", err);
        0
      });

  let workspace_limit = get_user_workspace_limit(
    &session.token.access_token,
    &state.appflowy_cloud_gateway_url,
  )
  .await
  .map(|limit| limit.workspace_count.to_string())
  .unwrap_or_else(|err| {
    tracing::warn!("unable to get user workspace limit: {:?}", err);
    "N/A".to_owned()
  });

  render_template(templates::UserUsage {
    workspace_count,
    workspace_limit,
  })
}

pub async fn workspace_usage_handler(
  State(app_state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let workspace_usages = get_user_workspace_usages(
    &session.token.access_token,
    &app_state.appflowy_cloud_url,
    &app_state.appflowy_cloud_gateway_url,
  )
  .await?;
  render_template(templates::WorkspaceUsageList { workspace_usages })
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
