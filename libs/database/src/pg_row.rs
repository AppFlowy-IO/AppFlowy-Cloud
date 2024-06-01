use anyhow::anyhow;
use app_error::AppError;
use chrono::{DateTime, Utc};

use database_entity::dto::{
  AFAccessLevel, AFRole, AFUserProfile, AFWorkspace, AFWorkspaceInvitationStatus,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Represent the row of the af_workspace table
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AFWorkspaceRow {
  pub workspace_id: Uuid,
  pub database_storage_id: Option<Uuid>,
  pub owner_uid: Option<i64>,
  pub owner_name: Option<String>,
  pub created_at: Option<DateTime<Utc>>,
  pub workspace_type: i32,
  pub deleted_at: Option<DateTime<Utc>>,
  pub workspace_name: Option<String>,
  pub icon: Option<String>,
}

impl TryFrom<AFWorkspaceRow> for AFWorkspace {
  type Error = AppError;

  fn try_from(value: AFWorkspaceRow) -> Result<Self, Self::Error> {
    let owner_uid = value
      .owner_uid
      .ok_or(AppError::Internal(anyhow!("Unexpected empty owner_uid")))?;
    let database_storage_id = value
      .database_storage_id
      .ok_or(AppError::Internal(anyhow!("Unexpected empty workspace_id")))?;

    let workspace_name = value.workspace_name.unwrap_or_default();
    let created_at = value.created_at.unwrap_or_else(Utc::now);
    let icon = value.icon.unwrap_or_default();

    Ok(Self {
      workspace_id: value.workspace_id,
      database_storage_id,
      owner_uid,
      owner_name: value.owner_name.unwrap_or_default(),
      workspace_type: value.workspace_type,
      workspace_name,
      created_at,
      icon,
    })
  }
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

#[derive(Debug, FromRow)]
pub struct AFUserIdRow {
  pub uid: i64,
  pub uuid: Uuid,
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

impl TryFrom<AFUserProfileRow> for AFUserProfile {
  type Error = AppError;

  fn try_from(value: AFUserProfileRow) -> Result<Self, Self::Error> {
    let uid = value
      .uid
      .ok_or(AppError::Internal(anyhow!("Unexpected empty uid")))?;
    let uuid = value
      .uuid
      .ok_or(AppError::Internal(anyhow!("Unexpected empty uuid")))?;
    let latest_workspace_id = value.latest_workspace_id.ok_or(AppError::Internal(anyhow!(
      "Unexpected empty latest_workspace_id"
    )))?;
    Ok(Self {
      uid,
      uuid,
      email: value.email,
      password: value.password,
      name: value.name,
      metadata: value.metadata,
      encryption_sign: value.encryption_sign,
      latest_workspace_id,
      updated_at: value.updated_at.map(|v| v.timestamp()).unwrap_or(0),
    })
  }
}

#[derive(FromRow, Serialize, Deserialize)]
pub struct AFWorkspaceMemberPermRow {
  pub uid: i64,
  pub role: AFRole,
  pub workspace_id: Uuid,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct AFWorkspaceMemberRow {
  pub uid: i64,
  pub name: String,
  pub email: String,
  pub role: AFRole,
}

#[derive(FromRow)]
pub struct AFCollabMemberAccessLevelRow {
  pub uid: i64,
  pub oid: String,
  pub access_level: AFAccessLevel,
}

#[derive(FromRow, Clone, Debug, Serialize, Deserialize)]
pub struct AFCollabMemberRow {
  pub uid: i64,
  pub oid: String,
  pub permission_id: i64,
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
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

#[derive(FromRow, Debug, Clone)]
pub struct AFPermissionRow {
  pub id: i32,
  pub name: String,
  pub access_level: AFAccessLevel,
  pub description: Option<String>,
}

#[derive(FromRow, Serialize, Deserialize)]
pub struct AFSnapshotRow {
  pub sid: i64,
  pub oid: String,
  pub blob: Vec<u8>,
  pub len: Option<i32>,
  pub encrypt: Option<i32>,
  pub deleted_at: Option<DateTime<Utc>>,
  pub created_at: DateTime<Utc>,
  pub workspace_id: Uuid,
}

#[derive(Debug, FromRow, Deserialize, Serialize)]
pub struct AFWorkspaceInvitationMinimal {
  pub workspace_id: Uuid,
  pub inviter_uid: i64,
  pub invitee_uid: Option<i64>,
  pub status: AFWorkspaceInvitationStatus,
  pub role: AFRole,
}

#[derive(FromRow, Clone, Debug)]
pub struct AFCollabRowMeta {
  pub oid: String,
  pub workspace_id: Uuid,

  pub deleted_at: Option<DateTime<Utc>>,
  pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AFChatRow {
  pub chat_id: Uuid,
  pub name: String,
  pub created_at: DateTime<Utc>,
  pub deleted_at: Option<DateTime<Utc>>,
  pub rag_ids: serde_json::Value,
  pub workspace_id: Uuid,
  pub meta_data: serde_json::Value,
}
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AFChatMessageRow {
  pub message_id: i64,
  pub chat_id: Uuid,
  pub content: String,
  pub created_at: DateTime<Utc>,
}
