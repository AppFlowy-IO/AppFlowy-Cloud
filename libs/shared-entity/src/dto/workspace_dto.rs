use chrono::{DateTime, Utc};
use database_entity::dto::AFRole;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
pub struct WorkspaceMembers(pub Vec<WorkspaceMember>);
#[derive(Deserialize, Serialize)]
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

#[derive(Deserialize, Serialize)]
pub struct CreateWorkspaceMembers(pub Vec<CreateWorkspaceMember>);
impl From<Vec<CreateWorkspaceMember>> for CreateWorkspaceMembers {
  fn from(value: Vec<CreateWorkspaceMember>) -> Self {
    Self(value)
  }
}

#[derive(Deserialize, Serialize)]
pub struct CreateWorkspaceMember {
  pub email: String,
  pub role: AFRole,
}

#[derive(Deserialize, Serialize)]
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
  pub fn with_role<T: Into<AFRole>>(mut self, role: T) -> Self {
    self.role = Some(role.into());
    self
  }
  pub fn with_name(mut self, name: String) -> Self {
    self.name = Some(name);
    self
  }
}

#[derive(Deserialize, Serialize)]
pub struct WorkspaceSpaceUsage {
  pub total_capacity: u64,
  pub consumed_capacity: u64,
}

#[derive(Serialize, Deserialize)]
pub struct RepeatedBlobMetaData(pub Vec<BlobMetadata>);

#[derive(Serialize, Deserialize)]
pub struct BlobMetadata {
  pub workspace_id: Uuid,
  pub file_id: String,
  pub file_type: String,
  pub file_size: i64,
  pub modified_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
pub struct CreateWorkspaceParam {
  pub workspace_name: Option<String>,
}
