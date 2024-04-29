use serde::Deserialize;

#[derive(Deserialize)]
pub struct WebApiLoginRequest {
  pub email: String,
  pub password: String,
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
  pub workspace_invitation_id: Option<String>,
  pub workspace_name: Option<String>,
  pub workspace_icon: Option<String>,
  pub user_name: Option<String>,
  pub user_icon: Option<String>,
  pub workspace_member_count: Option<String>,

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
