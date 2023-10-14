use database_entity::AFRole;
use std::ops::Deref;

#[derive(serde::Deserialize, serde::Serialize)]
pub struct WorkspaceMembers(pub Vec<WorkspaceMember>);
#[derive(serde::Deserialize, serde::Serialize)]
pub struct WorkspaceMember(pub String);
impl Deref for WorkspaceMember {
  type Target = String;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl From<Vec<String>> for WorkspaceMembers {
  fn from(value: Vec<String>) -> Self {
    Self(value.into_iter().map(WorkspaceMember).collect())
  }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct CreateWorkspaceMembers(pub Vec<CreateWorkspaceMember>);
impl From<Vec<CreateWorkspaceMember>> for CreateWorkspaceMembers {
  fn from(value: Vec<CreateWorkspaceMember>) -> Self {
    Self(value)
  }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct CreateWorkspaceMember {
  pub email: String,
  pub role: AFRole,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct WorkspaceMemberChangeset {
  pub email: String,
  pub role: Option<AFRole>,
  pub name: Option<String>,
}

impl WorkspaceMemberChangeset {
  pub fn new(email: String) -> Self {
    Self {
      email,
      role: None,
      name: None,
    }
  }

  pub fn with_role(mut self, role: AFRole) -> Self {
    self.role = Some(role);
    self
  }

  pub fn with_name(mut self, name: String) -> Self {
    self.name = Some(name);
    self
  }
}
