use chrono::{DateTime, Utc};
use collab_entity::{CollabType, EncodedCollab};
use database_entity::dto::{AFRole, AFWorkspaceInvitationStatus};
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

// Deprecated
#[derive(Deserialize, Serialize)]
pub struct CreateWorkspaceMember {
  pub email: String,
  pub role: AFRole,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkspaceMemberInvitation {
  pub email: String,
  pub role: AFRole,
}

#[derive(Deserialize)]
pub struct WorkspaceInviteQuery {
  pub status: Option<AFWorkspaceInvitationStatus>,
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

#[derive(Serialize, Deserialize, Default)]
pub struct PatchWorkspaceParam {
  pub workspace_id: Uuid,
  pub workspace_name: Option<String>,
  pub workspace_icon: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct CollabTypeParam {
  pub collab_type: CollabType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollabResponse {
  #[serde(flatten)]
  pub encode_collab: EncodedCollab,
  /// Object ID is marked with `serde(default)` to handle cases where `object_id` is missing in the data.
  /// This scenario can occur if the server data does not include `object_id` due to version downgrades (pre-0325 versions).
  /// The default ensures graceful handling of missing `object_id` during deserialization, preventing errors in client applications
  /// that expect this field to exist.
  ///
  /// We can remove this 'serde(default)' after the 0325 version is stable.
  #[serde(default)]
  pub object_id: String,
}
