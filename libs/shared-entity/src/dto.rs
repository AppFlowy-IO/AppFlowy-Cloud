// Data Transfer Objects (DTO)

use gotrue_entity::{AccessTokenResponse, User};

#[derive(serde::Deserialize, serde::Serialize)]
pub struct WorkspaceMembersParams {
  pub workspace_uuid: uuid::Uuid,
  pub member_emails: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInParams {
  pub email: String,
  pub password: String,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct UserUpdateParams {
  pub name: Option<String>,
  pub email: Option<String>,
  pub password: Option<String>,
}

impl UserUpdateParams {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_name<T: ToString>(mut self, name: T) -> Self {
    self.name = Some(name.to_string());
    self
  }

  pub fn with_email<T: ToString>(mut self, email: T) -> Self {
    self.email = Some(email.to_string());
    self
  }

  pub fn with_password(mut self, password: &str) -> Self {
    self.password = Some(password.to_owned());
    self
  }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInPasswordResponse {
  pub access_token_resp: AccessTokenResponse,
  pub is_new: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInTokenResponse {
  pub user: User,
  pub is_new: bool,
}
