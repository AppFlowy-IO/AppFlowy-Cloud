use crate::dto::AFRole;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Represent the row of the af_workspace table
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AFWorkspaceRow {
  pub workspace_id: Uuid,
  pub database_storage_id: Option<Uuid>,
  pub owner_uid: Option<i64>,
  pub created_at: Option<DateTime<Utc>>,
  pub workspace_type: i32,
  pub deleted_at: Option<DateTime<Utc>>,
  pub workspace_name: Option<String>,
}

/// Represent the row of the af_user table
#[derive(Debug, FromRow, Deserialize, Serialize, Clone)]
pub struct AFUserRow {
  pub uid: i64,
  pub uuid: Option<Uuid>,
  pub email: Option<String>,
  pub password: Option<String>,
  pub name: Option<String>,
  pub metadata: Option<serde_json::Value>,
  pub encryption_sign: Option<String>,
  pub deleted_at: Option<DateTime<Utc>>,
  pub updated_at: Option<DateTime<Utc>>,
  pub created_at: Option<DateTime<Utc>>,
}

/// Represent the row of the af_user_profile_view
#[derive(Debug, FromRow, Deserialize, Serialize)]
pub struct AFUserProfileRow {
  pub uid: Option<i64>,
  pub uuid: Option<Uuid>,
  pub email: Option<String>,
  pub password: Option<String>,
  pub name: Option<String>,
  pub metadata: Option<serde_json::Value>,
  pub encryption_sign: Option<String>,
  pub deleted_at: Option<DateTime<Utc>>,
  pub updated_at: Option<DateTime<Utc>>,
  pub created_at: Option<DateTime<Utc>>,
  pub latest_workspace_id: Option<Uuid>,
}

#[derive(FromRow, Serialize, Deserialize)]
pub struct AFWorkspaceMemberRow {
  pub name: String,
  pub email: String,
  pub role: AFRole,
}

#[derive(FromRow, Clone, Debug, Serialize, Deserialize)]
pub struct AFCollabMemberRow {
  pub uid: i64,
  pub oid: String,
  pub permission_id: i64,
}

#[derive(FromRow, Serialize, Deserialize)]
pub struct AFBlobMetadataRow {
  pub workspace_id: Uuid,
  pub file_id: String,
  pub file_type: String,
  pub file_size: i64,
  pub modified_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AFUserNotification {
  pub payload: Option<AFUserRow>,
}
