use serde::{Deserialize, Serialize};

use crate::{config::Config, session};

#[derive(Clone)]
pub struct AppState {
  pub appflowy_cloud_url: String,
  pub gotrue_client: gotrue::api::Client,
  pub session_store: session::SessionStorage,
  pub config: Config,
}

impl AppState {
  pub fn prepend_with_path_prefix(&self, path: &str) -> String {
    format!("{}{}", self.config.path_prefix, path)
  }
}

#[derive(Serialize, Deserialize)]
pub struct WebApiLoginRequest {
  pub email: String,
  pub password: String,
  pub redirect_to: Option<String>,
}

#[derive(Deserialize)]
pub struct WebApiPutUserRequest {
  pub password: String,
}

#[derive(Deserialize)]
pub struct WebApiChangePasswordRequest {
  pub new_password: String,
  pub confirm_password: String,
}

#[derive(Deserialize)]
pub struct WebApiAdminCreateUserRequest {
  pub email: String,
  pub password: String,
  pub require_email_verification: bool,
}

#[derive(Deserialize)]
pub struct WebApiInviteUserRequest {
  pub email: String,
}

#[derive(Deserialize)]
pub struct WebApiCreateSSOProviderRequest {
  #[serde(rename = "type")]
  pub type_: String,
  pub metadata_url: String,
}

#[derive(Deserialize)]
pub struct WebAppOAuthLoginRequest {
  // Use for Login
  pub refresh_token: Option<String>,

  // Use actions (with params) after login
  pub action: Option<OAuthLoginAction>,

  // Workspace Invitation
  pub workspace_invitation_id: Option<String>,
  pub workspace_name: Option<String>,
  pub workspace_icon: Option<String>,
  pub user_name: Option<String>,
  pub user_icon: Option<String>,
  pub workspace_member_count: Option<String>,

  // Redirect
  pub redirect_to: Option<String>,

  // Errors
  pub error: Option<String>,
  pub error_code: Option<i64>,
  pub error_description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthLoginAction {
  AcceptWorkspaceInvite,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthRedirect {
  pub client_id: String,
  pub state: String,
  pub redirect_uri: String,
  pub response_type: String,
  // pub scope: Option<String>,
  pub code_challenge: Option<String>,
  pub code_challenge_method: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OAuthRedirectToken {
  pub code: String,
  pub client_id: Option<String>,
  pub client_secret: Option<String>,
  pub grant_type: String,
  pub redirect_uri: Option<String>,
  pub code_verifier: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginParams {
  pub redirect_to: Option<String>,
}
