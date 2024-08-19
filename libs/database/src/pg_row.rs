use anyhow::anyhow;
use app_error::AppError;
use chrono::{DateTime, Utc};

use database_entity::dto::{
  AFAccessLevel, AFRole, AFUserProfile, AFWebUser, AFWorkspace, AFWorkspaceInvitationStatus,
  AccountLink, GlobalComment, Reaction, TemplateCategory, TemplateCategoryType, TemplateCreator,
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
      member_count: None,
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

#[derive(sqlx::Type, Serialize, Debug)]
pub struct AFWebUserColumn {
  uuid: Uuid,
  name: String,
  avatar_url: Option<String>,
}

impl From<AFWebUserColumn> for AFWebUser {
  fn from(val: AFWebUserColumn) -> Self {
    AFWebUser {
      uuid: val.uuid,
      name: val.name,
      avatar_url: val.avatar_url,
    }
  }
}

pub struct AFGlobalCommentRow {
  pub user: Option<AFWebUserColumn>,
  pub created_at: DateTime<Utc>,
  pub last_updated_at: DateTime<Utc>,
  pub content: String,
  pub reply_comment_id: Option<Uuid>,
  pub comment_id: Uuid,
  pub is_deleted: bool,
  pub can_be_deleted: bool,
}

impl From<AFGlobalCommentRow> for GlobalComment {
  fn from(val: AFGlobalCommentRow) -> Self {
    GlobalComment {
      user: val.user.map(|x| x.into()),
      created_at: val.created_at,
      last_updated_at: val.last_updated_at,
      content: val.content,
      reply_comment_id: val.reply_comment_id,
      comment_id: val.comment_id,
      is_deleted: val.is_deleted,
      can_be_deleted: val.can_be_deleted,
    }
  }
}

pub struct AFReactionRow {
  pub reaction_type: String,
  pub react_users: Vec<AFWebUserColumn>,
  pub comment_id: Uuid,
}

impl From<AFReactionRow> for Reaction {
  fn from(val: AFReactionRow) -> Self {
    Reaction {
      reaction_type: val.reaction_type,
      react_users: val.react_users.into_iter().map(|x| x.into()).collect(),
      comment_id: val.comment_id,
    }
  }
}

#[derive(Debug, FromRow, Serialize)]
pub struct AFTemplateCategoryRow {
  pub id: Uuid,
  pub name: String,
  pub icon: String,
  pub bg_color: String,
  pub description: String,
  pub category_type: AFTemplateCategoryTypeColumn,
  pub priority: i32,
}

impl From<AFTemplateCategoryRow> for TemplateCategory {
  fn from(value: AFTemplateCategoryRow) -> Self {
    Self {
      id: value.id,
      name: value.name,
      icon: value.icon,
      bg_color: value.bg_color,
      description: value.description,
      category_type: value.category_type.into(),
      priority: value.priority,
    }
  }
}

#[derive(sqlx::Type, Serialize, Debug)]
#[repr(i32)]
pub enum AFTemplateCategoryTypeColumn {
  UseCase = 0,
  Feature = 1,
}

impl From<AFTemplateCategoryTypeColumn> for TemplateCategoryType {
  fn from(value: AFTemplateCategoryTypeColumn) -> Self {
    match value {
      AFTemplateCategoryTypeColumn::UseCase => TemplateCategoryType::UseCase,
      AFTemplateCategoryTypeColumn::Feature => TemplateCategoryType::Feature,
    }
  }
}

impl From<TemplateCategoryType> for AFTemplateCategoryTypeColumn {
  fn from(val: TemplateCategoryType) -> Self {
    match val {
      TemplateCategoryType::UseCase => AFTemplateCategoryTypeColumn::UseCase,
      TemplateCategoryType::Feature => AFTemplateCategoryTypeColumn::Feature,
    }
  }
}

#[derive(sqlx::Type, Serialize, Debug)]
pub struct AccountLinkColumn {
  pub link_type: String,
  pub url: String,
}

impl From<AccountLinkColumn> for AccountLink {
  fn from(value: AccountLinkColumn) -> Self {
    Self {
      link_type: value.link_type,
      url: value.url,
    }
  }
}

#[derive(Debug, Serialize)]
pub struct AFTemplateCreatorRow {
  pub id: Uuid,
  pub name: String,
  pub avatar_url: String,
  pub account_links: Option<Vec<AccountLinkColumn>>,
}

impl From<AFTemplateCreatorRow> for TemplateCreator {
  fn from(value: AFTemplateCreatorRow) -> Self {
    let account_links = value
      .account_links
      .unwrap_or_default()
      .into_iter()
      .map(|v| v.into())
      .collect();
    Self {
      id: value.id,
      name: value.name,
      avatar_url: value.avatar_url,
      account_links,
    }
  }
}
