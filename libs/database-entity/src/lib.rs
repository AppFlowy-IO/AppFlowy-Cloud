use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::ops::Deref;
pub mod error;

pub type RawData = Vec<u8>;

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

#[derive(Serialize, Deserialize)]
pub enum AFRole {
  Owner,
  Member,
  Guest,
}

impl AFRole {
  pub fn id(&self) -> i32 {
    match self {
      AFRole::Owner => 1,
      AFRole::Member => 2,
      AFRole::Guest => 3,
    }
  }
}

impl From<i32> for AFRole {
  fn from(item: i32) -> Self {
    match item {
      1 => AFRole::Owner,
      2 => AFRole::Member,
      3 => AFRole::Guest,
      _ => panic!("Invalid value for AFRole"),
    }
  }
}

#[derive(FromRow, Serialize, Deserialize)]
pub struct AFWorkspaceMember {
  pub email: String,
  pub role: AFRole,
}

#[derive(FromRow, Serialize, Deserialize)]
pub struct AFFileMetadata {
  pub owner_uid: i64,
  pub path: String,
  pub file_type: String,
  pub file_size: i64,
  pub created_at: DateTime<Utc>,
}

impl AFFileMetadata {
  pub fn s3_path(&self) -> String {
    format!("{}/{}", self.owner_uid, self.path)
  }
}
