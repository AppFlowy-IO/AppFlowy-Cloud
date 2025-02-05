use crate::askama_entities::WorkspaceWithMembers;
use crate::error::WebAppError;
use crate::ext::api::{
  accept_workspace_invitation, get_accepted_workspace_invitations,
  get_pending_workspace_invitations, get_user_owned_workspaces, get_user_profile,
  get_user_workspace_limit, get_user_workspace_usages, get_user_workspaces, get_workspace_members,
  verify_token_cloud,
};
use crate::models::{LoginParams, OAuthLoginAction, WebAppOAuthLoginRequest};
use crate::session::{self, new_session_cookie, UserSession};
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Result};
use axum::{response::Html, routing::get, Router};
use axum_extra::extract::CookieJar;
use gotrue_entity::dto::User;

use crate::{templates, AppState};

static DEFAULT_OAUTH_REDIRECT_TO_WITHOUT_PREFIX: &str = "/web/login-callback";

pub fn router(state: AppState) -> Router<AppState> {
  Router::new()
    .nest_service("/", page_router().with_state(state.clone()))
    .nest_service("/components", component_router().with_state(state))
}

fn page_router() -> Router<AppState> {
  Router::new()
    .route("/", get(home_handler))
    .route("/login", get(login_handler))
    .route("/login-v2", get(login_v2_handler))
    .route("/login-callback", get(login_callback_handler))
    .route("/payment-success", get(payment_success_handler))
    .route("/login-callback-query", get(login_callback_query_handler))
    .route(
      "/open-appflowy-or-download",
      get(open_appflowy_or_download_handler),
    )
    .route("/home", get(home_handler))
    .route("/admin/home", get(admin_home_handler))
}

fn component_router() -> Router<AppState> {
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

async fn open_appflowy_or_download_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::OpenAppFlowyOrDownload {})
}

async fn login_callback_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::LoginCallback {})
}

async fn payment_success_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::PaymentSuccessRedirect {})
}

async fn login_callback_query_handler(
  State(state): State<AppState>,
  session: Option<UserSession>,
  Query(query): Query<WebAppOAuthLoginRequest>,
  mut jar: CookieJar,
) -> Result<axum::response::Response, WebAppError> {
  let refresh_token = {
    match query.refresh_token {
      Some(refresh_token) => refresh_token,
      None => match session {
        Some(session) => session.token.refresh_token,
        None => match query.error {
          Some(err) => {
            tracing::error!(
              "OAuth login error: {:?}, code: {:?}, description: {:?}",
              err,
              query.error_code,
              query.error_description
            );
            let redirect_url = format!(
                "https://appflowy.io/invitation/expired?workspace_name={}&workspace_icon={}&user_name={}&user_icon={}&workspace_member_count={}",
                query.workspace_name.unwrap_or_default(),
                query.workspace_icon.unwrap_or_default(),
                query.user_name.unwrap_or_default(),
                query.user_icon.unwrap_or_default(),
                query.workspace_member_count.unwrap_or_default());

            let expired_html = render_template(templates::Redirect { redirect_url })?;
            return Ok(expired_html.into_response());
          },
          None => {
            return Err(WebAppError::BadRequest(
              "refresh_token not found".to_string(),
            ));
          },
        },
      },
    }
  };

  let token = state
    .gotrue_client
    .token(&gotrue::grant::Grant::RefreshToken(
      gotrue::grant::RefreshTokenGrant { refresh_token },
    ))
    .await
    .map_err(|_| WebAppError::LoginRedirectRequired(state.config.path_prefix.clone()))?;

  verify_token_cloud(
    token.access_token.as_str(),
    state.appflowy_cloud_url.as_str(),
  )
  .await?;

  let new_session_id = uuid::Uuid::new_v4();
  let new_session = session::UserSession {
    session_id: new_session_id.to_string(),
    token,
  };
  state.session_store.put_user_session(&new_session).await?;
  jar = jar.add(new_session_cookie(new_session_id));

  match query.action {
    Some(action) => match action {
      OAuthLoginAction::AcceptWorkspaceInvite => {
        let invite_id = query
          .workspace_invitation_id
          .ok_or(WebAppError::BadRequest(
            "workspace_invitation_id not found".to_string(),
          ))?;

        {
          // If user has already accepted the invitation, redirect to open or download AppFlowy
          let accepted_invitations = get_accepted_workspace_invitations(
            &new_session.token.access_token,
            &state.appflowy_cloud_url,
          )
          .await?;
          let found = accepted_invitations
            .iter()
            .find(|w| w.invite_id.to_string() == invite_id);
          if found.is_some() {
            let open_or_dl_html = render_template(templates::OpenAppFlowyOrDownload {})?;
            return Ok((jar, open_or_dl_html).into_response());
          }
        }

        if let Err(err) = accept_workspace_invitation(
          &new_session.token.access_token,
          &invite_id,
          &state.appflowy_cloud_url,
        )
        .await
        {
          tracing::error!("accepting workspace invitation: {:?}", err);
          let redirect_url = format!(
            "https://appflowy.io/invitation/expired?workspace_name={}&workspace_icon={}&user_name={}&user_icon={}&workspace_member_count={}",
            query.workspace_name.unwrap_or_default(),
            query.workspace_icon.unwrap_or_default(),
            query.user_name.unwrap_or_default(),
            query.user_icon.unwrap_or_default(),
            query.workspace_member_count.unwrap_or_default());
          let redirect_html = render_template(templates::Redirect { redirect_url })?;
          return Ok(redirect_html.into_response());
        };
        let open_or_dl_html = render_template(templates::OpenAppFlowyOrDownload {})?;
        Ok((jar, open_or_dl_html).into_response())
      },
    },
    None => match query.redirect_to {
      Some(redirect_url) => match urlencoding::decode(&redirect_url).map(String::from) {
        Ok(redirect_url) => {
          let redirect_html = render_template(templates::Redirect { redirect_url })?;
          Ok((jar, redirect_html).into_response())
        },
        Err(err) => {
          tracing::error!("Error decoding redirect_url: {:?}", err);
          home_handler(State(state), Some(new_session), jar).await
        },
      },
      None => home_handler(State(state), Some(new_session), jar).await,
    },
  }
}

async fn admin_sso_detail_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(sso_provider_id): Path<String>,
) -> Result<Html<String>, WebAppError> {
  let sso_provider = state
    .gotrue_client
    .admin_get_sso_provider(&session.token.access_token, &sso_provider_id)
    .await
    .map_err(|_| WebAppError::LoginRedirectRequired(state.config.path_prefix.clone()))?;

  let mapping_json =
    serde_json::to_string_pretty(&sso_provider.saml.attribute_mapping).unwrap_or("".to_owned());

  render_template(templates::SsoDetail {
    sso_provider,
    mapping_json,
  })
}

async fn admin_sso_create_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::SsoCreate)
}

async fn admin_sso_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let sso_providers = state
    .gotrue_client
    .admin_list_sso_providers(&session.token.access_token)
    .await
    .map_err(|_| WebAppError::LoginRedirectRequired(state.config.path_prefix.clone()))?
    .items
    .unwrap_or_default();

  render_template(templates::SsoList { sso_providers })
}

async fn user_navigate_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::Navigate)
}

async fn admin_navigate_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::AdminNavigate)
}

async fn shared_workspaces_handler(
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

async fn user_invite_handler(
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

async fn user_usage_handler(
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

  let workspace_limit =
    get_user_workspace_limit(&session.token.access_token, &state.appflowy_cloud_url)
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

async fn workspace_usage_handler(
  State(app_state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let workspace_usages =
    get_user_workspace_usages(&session.token.access_token, &app_state.appflowy_cloud_url).await?;
  render_template(templates::WorkspaceUsageList { workspace_usages })
}

async fn admin_users_create_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::CreateUser)
}

async fn user_user_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let user = state
    .gotrue_client
    .user_info(&session.token.access_token)
    .await
    .map_err(|_| WebAppError::LoginRedirectRequired(state.config.path_prefix.clone()))?;
  render_template(templates::UserDetails { user: &user })
}

async fn login_handler(
  State(state): State<AppState>,
  Query(login): Query<LoginParams>,
) -> Result<Html<String>, WebAppError> {
  let redirect_to = login
    .redirect_to
    .as_ref()
    .map(|r| urlencoding::encode(r).to_string());
  let oauth_redirect_to = login.redirect_to.as_ref().map(|r| {
    urlencoding::encode(&format!(
      "{}/web/login-callback?redirect_to={}",
      state.config.path_prefix,
      urlencoding::encode(r)
    ))
    .to_string()
  });

  let external = state
    .gotrue_client
    .settings()
    .await
    .map_err(|_| WebAppError::LoginRedirectRequired(state.config.path_prefix.clone()))?
    .external;
  let oauth_providers = external.oauth_providers();
  let default_oauth_redirect_to = format!(
    "{}{}",
    state.config.path_prefix, DEFAULT_OAUTH_REDIRECT_TO_WITHOUT_PREFIX
  );
  render_template(templates::Login {
    path_prefix: &state.config.path_prefix,
    oauth_providers: &oauth_providers,
    redirect_to: redirect_to.as_deref(),
    oauth_redirect_to: oauth_redirect_to
      .as_deref()
      .unwrap_or(&default_oauth_redirect_to),
  })
}

async fn login_v2_handler(
  State(state): State<AppState>,
  Query(login): Query<LoginParams>,
) -> Result<Html<String>, WebAppError> {
  let redirect_to = login
    .redirect_to
    .as_ref()
    .map(|r| urlencoding::encode(r).to_string());
  let oauth_redirect_to = login.redirect_to.as_ref().map(|r| {
    urlencoding::encode(&format!(
      "{}/web/login-callback?redirect_to={}",
      state.config.path_prefix,
      urlencoding::encode(r)
    ))
    .to_string()
  });

  let default_oauth_redirect_to = format!(
    "{}{}",
    state.config.path_prefix, DEFAULT_OAUTH_REDIRECT_TO_WITHOUT_PREFIX
  );
  render_template(templates::LoginV2 {
    oauth_providers: &["Google", "Apple", "Github", "Discord"],
    redirect_to: redirect_to.as_deref(),
    oauth_redirect_to: oauth_redirect_to
      .as_deref()
      .unwrap_or(&default_oauth_redirect_to),
    path_prefix: &state.config.path_prefix,
  })
}

async fn user_change_password_handler() -> Result<Html<String>, WebAppError> {
  render_template(templates::ChangePassword)
}

pub async fn home_handler(
  State(state): State<AppState>,
  session: Option<UserSession>,
  jar: CookieJar,
) -> Result<axum::response::Response, WebAppError> {
  let redirect_url = state.prepend_with_path_prefix("/web/login");
  let session = match session {
    Some(session) => session,
    None => return Ok(Redirect::to(&redirect_url).into_response()),
  };

  let user = state
    .gotrue_client
    .user_info(&session.token.access_token)
    .await
    .map_err(|_| WebAppError::LoginRedirectRequired(state.config.path_prefix.clone()))?;
  let home_html_str = render_template(templates::Home {
    user: &user,
    is_admin: is_admin(&user),
    path_prefix: &state.config.path_prefix,
  })?;
  Ok((jar, home_html_str).into_response())
}

async fn admin_home_handler(
  State(state): State<AppState>,
  session: UserSession,
) -> Result<Html<String>, WebAppError> {
  let user = state
    .gotrue_client
    .user_info(&session.token.access_token)
    .await
    .map_err(|_| WebAppError::LoginRedirectRequired(state.config.path_prefix.clone()))?;
  render_template(templates::AdminHome {
    user: &user,
    path_prefix: &state.config.path_prefix,
  })
}

async fn admin_users_handler(
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

async fn admin_user_details_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(user_id): Path<String>,
) -> Result<Html<String>, WebAppError> {
  let user = state
    .gotrue_client
    .admin_user_details(&session.token.access_token, &user_id)
    .await
    .map_err(|_| WebAppError::LoginRedirectRequired(state.config.path_prefix.clone()))?;

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
