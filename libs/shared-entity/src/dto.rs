// Data Transfer Objects (DTO)

use database_entity::AFRole;
use gotrue_entity::AccessTokenResponse;

#[derive(serde::Deserialize, serde::Serialize)]
pub struct WorkspaceMembers(pub Vec<String>);

#[derive(serde::Deserialize, serde::Serialize)]
pub struct CreateWorkspaceMembers(pub Vec<CreateWorkspaceMember>);

impl From<Vec<CreateWorkspaceMember>> for CreateWorkspaceMembers {
  fn from(value: Vec<CreateWorkspaceMember>) -> Self {
    Self(value)
  }
}

#[derive(serde_repr::Deserialize_repr, serde_repr::Serialize_repr)]
#[repr(u8)]
pub enum WorkspacePermission {
  Owner = 0,
  Member = 1,
  Guest = 2,
}

impl From<WorkspacePermission> for AFRole {
  fn from(value: WorkspacePermission) -> Self {
    match value {
      WorkspacePermission::Owner => AFRole::Owner,
      WorkspacePermission::Member => AFRole::Member,
      WorkspacePermission::Guest => AFRole::Guest,
    }
  }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct CreateWorkspaceMember {
  pub email: String,
  pub permission: WorkspacePermission,
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

#[derive(serde::Deserialize, serde::Serialize)]
pub struct UpdateUsernameParams {
  pub new_name: String,
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
  pub is_new: bool,
}
