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

#[derive(Deserialize, Debug)]
pub struct WebAppOAuthLoginRequest {
  // Use for Login
  pub refresh_token: String,

  // Use actions (with params) after login
  pub action: Option<String>,
  pub workspace_id: Option<String>,
}
