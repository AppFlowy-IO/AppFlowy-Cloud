use chrono::{DateTime, Utc};
use collab_entity::CollabType;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use uuid::Uuid;
use validator::{Validate, ValidationError};

pub type RawData = Vec<u8>;
pub mod database_error;

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
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct DeleteCollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
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

impl InsertCollabParams {
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
  pub collab_type: CollabType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchQueryCollabParams(pub Vec<QueryCollabParams>);

impl Deref for BatchQueryCollabParams {
  type Target = Vec<QueryCollabParams>;

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

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AFWorkspace {
  pub workspace_id: uuid::Uuid,
  pub database_storage_id: Option<uuid::Uuid>,
  pub owner_uid: Option<i64>,
  pub created_at: Option<DateTime<Utc>>,
  pub workspace_type: i32,
  pub deleted_at: Option<DateTime<Utc>>,
  pub workspace_name: Option<String>,
}

#[derive(Debug, FromRow, Deserialize, Serialize)]
pub struct AFUserProfileView {
  pub uid: Option<i64>,
  pub uuid: Option<uuid::Uuid>,
  pub email: Option<String>,
  pub password: Option<String>,
  pub name: Option<String>,
  pub encryption_sign: Option<String>,
  pub deleted_at: Option<DateTime<Utc>>,
  pub updated_at: Option<DateTime<Utc>>,
  pub created_at: Option<DateTime<Utc>>,
  pub latest_workspace_id: Option<uuid::Uuid>,
}

#[derive(Debug, FromRow, Deserialize, Serialize)]
pub struct AFWorkspaces(pub Vec<AFWorkspace>);

impl Deref for AFWorkspaces {
  type Target = Vec<AFWorkspace>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl From<Vec<AFWorkspace>> for AFWorkspaces {
  fn from(v: Vec<AFWorkspace>) -> Self {
    Self(v)
  }
}

impl AFWorkspaces {
  pub fn get_latest(&self, profile: &AFUserProfileView) -> Option<AFWorkspace> {
    match profile.latest_workspace_id {
      Some(ws_id) => self.0.iter().find(|ws| ws.workspace_id == ws_id).cloned(),
      None => None,
    }
  }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
pub enum AFRole {
  Owner,
  Member,
  Guest,
}

impl From<i32> for AFRole {
  fn from(value: i32) -> Self {
    // Can't modify the value of the enum
    match value {
      1 => AFRole::Owner,
      2 => AFRole::Member,
      3 => AFRole::Guest,
      _ => panic!("Invalid value for AFRole"),
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
#[derive(FromRow, Serialize, Deserialize)]
pub struct AFWorkspaceMember {
  pub email: String,
  pub role: AFRole,
}

#[derive(FromRow, Serialize, Deserialize)]
pub struct AFBlobMetadata {
  pub workspace_id: Uuid,
  pub file_id: String,
  pub file_type: String,
  pub file_size: i64,
  pub modified_at: DateTime<Utc>,
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

impl AFBlobMetadata {
  pub fn s3_path(&self) -> String {
    format!("{}/{}", self.workspace_id, self.file_id)
  }
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum QueryCollabResult {
  Success { blob: RawData },
  Failed { error: String },
}

#[derive(Serialize, Deserialize)]
pub struct BatchQueryCollabResult(pub HashMap<String, QueryCollabResult>);
