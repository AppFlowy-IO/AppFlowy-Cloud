use chrono::{DateTime, Utc};
use collab_entity::CollabType;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use tracing::error;
use uuid::Uuid;
use validator::{Validate, ValidationError};

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct CreateCollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,

  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,

  #[validate(custom = "validate_not_empty_payload")]
  pub encoded_collab_v1: Vec<u8>,

  pub collab_type: CollabType,

  /// Determine whether to override the collab if it exists. Default is false.
  #[serde(default)]
  pub override_if_exist: bool,
}

impl CreateCollabParams {
  pub fn split(self) -> (CollabParams, String) {
    (
      CollabParams {
        object_id: self.object_id,
        encoded_collab_v1: self.encoded_collab_v1,
        collab_type: self.collab_type,
        override_if_exist: self.override_if_exist,
      },
      self.workspace_id,
    )
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
    bincode::serialize(self)
  }
  pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
    bincode::deserialize(bytes)
  }
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct CollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  #[validate(custom = "validate_not_empty_payload")]
  pub encoded_collab_v1: Vec<u8>,
  pub collab_type: CollabType,
  /// Determine whether to override the collab if it exists. Default is false.
  #[serde(default)]
  pub override_if_exist: bool,
}

impl CollabParams {
  pub fn new<T: ToString>(
    object_id: T,
    collab_type: CollabType,
    encoded_collab_v1: Vec<u8>,
  ) -> Self {
    let object_id = object_id.to_string();
    Self {
      object_id,
      collab_type,
      encoded_collab_v1,
      override_if_exist: false,
    }
  }

  pub fn override_collab_if_exist(mut self, override_if_exist: bool) -> Self {
    self.override_if_exist = override_if_exist;
    self
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
    bincode::serialize(self)
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
    bincode::deserialize(bytes)
  }
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct BatchCreateCollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
  pub params_list: Vec<CollabParams>,
}

impl BatchCreateCollabParams {
  pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
    bincode::serialize(self)
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
    bincode::deserialize(bytes)
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
  pub encoded_collab_v1: Vec<u8>,
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotData {
  pub object_id: String,
  pub encoded_collab_v1: Vec<u8>,
  pub workspace_id: String,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct QuerySnapshotParams {
  pub snapshot_id: i64,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct QueryCollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,

  #[serde(flatten)]
  #[validate]
  pub inner: QueryCollab,
}

impl QueryCollabParams {
  pub fn new<T1: ToString, T2: ToString>(
    object_id: T1,
    collab_type: CollabType,
    workspace_id: T2,
  ) -> Self {
    let workspace_id = workspace_id.to_string();
    let object_id = object_id.to_string();
    let inner = QueryCollab {
      object_id,
      collab_type,
    };
    Self {
      workspace_id,
      inner,
    }
  }
}

impl Deref for QueryCollabParams {
  type Target = QueryCollab;

  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct QueryCollab {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  pub collab_type: CollabType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchQueryCollabParams(pub Vec<QueryCollab>);

impl Deref for BatchQueryCollabParams {
  type Target = Vec<QueryCollab>;

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
pub struct AFSnapshotMeta {
  pub snapshot_id: i64,
  pub object_id: String,
  pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AFSnapshotMetas(pub Vec<AFSnapshotMeta>);

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
  Success { encode_collab_v1: Vec<u8> },
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

impl From<i64> for AFRole {
  fn from(value: i64) -> Self {
    Self::from(value as i32)
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

#[derive(Deserialize_repr, Serialize_repr, Eq, PartialEq, Debug, Clone, Copy)]
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
        error!("Invalid access level: {}", value);
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

#[derive(Serialize, Deserialize)]
pub struct AFUserProfile {
  pub uid: i64,
  pub uuid: Uuid,
  pub email: Option<String>,
  pub password: Option<String>,
  pub name: Option<String>,
  pub metadata: Option<serde_json::Value>,
  pub encryption_sign: Option<String>,
  pub latest_workspace_id: Uuid,
  pub updated_at: i64,
}

#[derive(Serialize, Deserialize)]
pub struct AFWorkspace {
  pub workspace_id: Uuid,
  pub database_storage_id: Uuid,
  pub owner_uid: i64,
  pub workspace_type: i32,
  pub workspace_name: String,
  pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
pub struct AFWorkspaces(pub Vec<AFWorkspace>);

#[derive(Serialize, Deserialize)]
pub struct AFUserWorkspaceInfo {
  pub user_profile: AFUserProfile,
  pub visiting_workspace: AFWorkspace,
  pub workspaces: Vec<AFWorkspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AFWorkspaceMember {
  pub name: String,
  pub email: String,
  pub role: AFRole,
  pub avatar_url: Option<String>,
}

// pub type AFBlobMetadata = AFBlobMetadataRow;
