use crate::pg_row::{AFBlobMetadataRow, AFUserProfileRow, AFWorkspaceMemberRow, AFWorkspaceRows};
use chrono::{DateTime, Utc};
use collab_entity::CollabType;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use tracing::error;
use validator::{Validate, ValidationError};

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct InsertCollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  #[validate(custom = "validate_not_empty_payload")]
  pub raw_data: Vec<u8>,
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
  pub collab_type: CollabType,
}

impl InsertCollabParams {
  pub fn new<T: ToString>(
    object_id: T,
    collab_type: CollabType,
    raw_data: Vec<u8>,
    workspace_id: String,
  ) -> Self {
    let object_id = object_id.to_string();
    Self {
      object_id,
      collab_type,
      raw_data,
      workspace_id,
    }
  }
  pub fn from_raw_data(
    object_id: &str,
    collab_type: CollabType,
    raw_data: Vec<u8>,
    workspace_id: &str,
  ) -> Self {
    let object_id = object_id.to_string();
    let workspace_id = workspace_id.to_string();
    Self {
      object_id,
      collab_type,
      raw_data,
      workspace_id,
    }
  }
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct DeleteCollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
}

fn validate_not_empty_str(s: &str) -> Result<(), ValidationError> {
  if s.is_empty() {
    return Err(ValidationError::new("should not be empty string"));
  }
  Ok(())
}

fn validate_not_empty_payload(payload: &[u8]) -> Result<(), ValidationError> {
  if payload.is_empty() {
    return Err(ValidationError::new("should not be empty payload"));
  }
  Ok(())
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct InsertSnapshotParams {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  #[validate(custom = "validate_not_empty_payload")]
  pub raw_data: Vec<u8>,
  pub len: i32,
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct QueryCollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  pub workspace_id: String,
  pub collab_type: CollabType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchQueryCollabParams(pub Vec<BatchQueryCollab>);

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct BatchQueryCollab {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  pub collab_type: CollabType,
}
impl Deref for BatchQueryCollabParams {
  type Target = Vec<BatchQueryCollab>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for BatchQueryCollabParams {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AFCollabSnapshot {
  pub snapshot_id: i64,
  pub object_id: String,
  pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AFCollabSnapshots(pub Vec<AFCollabSnapshot>);

#[derive(Debug, Clone, Deserialize)]
pub struct QuerySnapshotParams {
  pub snapshot_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QueryObjectSnapshotParams {
  pub object_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AFBlobRecord {
  pub file_id: String,
}

impl AFBlobRecord {
  pub fn new(file_id: String) -> Self {
    Self { file_id }
  }
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum QueryCollabResult {
  Success { blob: RawData },
  Failed { error: String },
}

#[derive(Serialize, Deserialize)]
pub struct BatchQueryCollabResult(pub HashMap<String, QueryCollabResult>);

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct InsertCollabMemberParams {
  pub uid: i64,
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  pub access_level: AFAccessLevel,
}

pub type UpdateCollabMemberParams = InsertCollabMemberParams;

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct CollabMemberIdentify {
  pub uid: i64,
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct QueryCollabMembers {
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct AFCollabMember {
  pub uid: i64,
  pub oid: String,
  pub permission: AFPermission,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
pub enum AFRole {
  Owner,
  Member,
  Guest,
}

impl AFRole {
  /// The user can create a [Collab] if the user is [AFRole::Owner] or [AFRole::Member] of the workspace.
  pub fn can_create_collab(&self) -> bool {
    matches!(self, AFRole::Owner | AFRole::Member)
  }
}

impl From<i32> for AFRole {
  fn from(value: i32) -> Self {
    // Can't modify the value of the enum
    match value {
      1 => AFRole::Owner,
      2 => AFRole::Member,
      3 => AFRole::Guest,
      _ => {
        error!("Invalid role id: {}", value);
        AFRole::Guest
      },
    }
  }
}

impl From<Option<i32>> for AFRole {
  fn from(value: Option<i32>) -> Self {
    match value {
      None => {
        error!("Invalid role id: None");
        AFRole::Guest
      },
      Some(value) => value.into(),
    }
  }
}

impl From<AFRole> for i32 {
  fn from(role: AFRole) -> Self {
    // Can't modify the value of the enum
    match role {
      AFRole::Owner => 1,
      AFRole::Member => 2,
      AFRole::Guest => 3,
    }
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AFPermission {
  /// The permission id
  pub id: i32,
  pub name: String,
  pub access_level: AFAccessLevel,
  pub description: String,
}

#[derive(Deserialize_repr, Serialize_repr, Eq, PartialEq, Debug, Clone)]
#[repr(i32)]
pub enum AFAccessLevel {
  // Can't modify the value of the enum
  ReadOnly = 10,
  ReadAndComment = 20,
  ReadAndWrite = 30,
  FullAccess = 50,
}

impl AFAccessLevel {
  pub fn can_write(&self) -> bool {
    match self {
      AFAccessLevel::ReadOnly | AFAccessLevel::ReadAndComment => false,
      AFAccessLevel::ReadAndWrite | AFAccessLevel::FullAccess => true,
    }
  }

  pub fn can_delete(&self) -> bool {
    match self {
      AFAccessLevel::ReadOnly | AFAccessLevel::ReadAndComment | AFAccessLevel::ReadAndWrite => {
        false
      },
      AFAccessLevel::FullAccess => true,
    }
  }
}

impl From<i32> for AFAccessLevel {
  fn from(value: i32) -> Self {
    // Can't modify the value of the enum
    match value {
      10 => AFAccessLevel::ReadOnly,
      20 => AFAccessLevel::ReadAndComment,
      30 => AFAccessLevel::ReadAndWrite,
      50 => AFAccessLevel::FullAccess,
      _ => {
        error!("Invalid role id: {}", value);
        AFAccessLevel::ReadOnly
      },
    }
  }
}

impl From<AFAccessLevel> for i32 {
  fn from(level: AFAccessLevel) -> Self {
    level as i32
  }
}

#[derive(Serialize, Deserialize)]
pub struct AFCollabMembers(pub Vec<AFCollabMember>);

pub type RawData = Vec<u8>;

/// ***************************************************************
/// Make alias for the database entity. Hiding the Sqlx Rows type.
pub type AFUserProfile = AFUserProfileRow;
pub type AFWorkspaces = AFWorkspaceRows;
pub type AFWorkspaceMember = AFWorkspaceMemberRow;
pub type AFBlobMetadata = AFBlobMetadataRow;
