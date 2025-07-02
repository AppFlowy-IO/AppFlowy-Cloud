use anyhow::anyhow;
use app_error::AppError;
use chrono::{DateTime, Utc};

use database_entity::dto::{
  AFAccessLevel, AFRole, AFUserProfile, AFWebUser, AFWebUserWithObfuscatedName, AFWorkspace,
  AFWorkspaceInvitationStatus, AFWorkspaceMember, AccessRequestMinimal, AccessRequestStatus,
  AccessRequestWithViewId, AccessRequesterInfo, AccountLink, GlobalComment, QuickNote, Reaction,
  Template, TemplateCategory, TemplateCategoryMinimal, TemplateCategoryType, TemplateCreator,
  TemplateCreatorMinimal, TemplateGroup, TemplateMinimal,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Represent the row of the af_workspace table
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, sqlx::Type)]
pub struct AFWorkspaceRow {
  pub workspace_id: Uuid,
  pub database_storage_id: Option<Uuid>,
  pub owner_uid: Option<i64>,
  pub owner_name: Option<String>,
  pub owner_email: Option<String>,
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
      owner_email: value.owner_email.unwrap_or_default(),
      workspace_type: value.workspace_type,
      workspace_name,
      created_at,
      icon,
      member_count: None,
      role: None,
    })
  }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AFWorkspaceRowWithMemberCountAndRole {
  pub workspace_id: Uuid,
  pub database_storage_id: Option<Uuid>,
  pub owner_uid: Option<i64>,
  pub owner_name: Option<String>,
  pub owner_email: Option<String>,
  pub created_at: Option<DateTime<Utc>>,
  pub workspace_type: i32,
  pub deleted_at: Option<DateTime<Utc>>,
  pub workspace_name: Option<String>,
  pub icon: Option<String>,
  pub member_count: i64,
  pub role: i32,
}

impl TryFrom<AFWorkspaceRowWithMemberCountAndRole> for AFWorkspace {
  type Error = AppError;

  fn try_from(value: AFWorkspaceRowWithMemberCountAndRole) -> Result<Self, Self::Error> {
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
      owner_email: value.owner_email.unwrap_or_default(),
      workspace_type: value.workspace_type,
      workspace_name,
      created_at,
      icon,
      member_count: Some(value.member_count),
      role: Some(AFRole::from(value.role)),
    })
  }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, sqlx::Type)]
pub struct AFWorkspaceWithMemberCountRow {
  pub workspace_id: Uuid,
  pub database_storage_id: Option<Uuid>,
  pub owner_uid: Option<i64>,
  pub owner_name: Option<String>,
  pub owner_email: Option<String>,
  pub created_at: Option<DateTime<Utc>>,
  pub workspace_type: i32,
  pub deleted_at: Option<DateTime<Utc>>,
  pub workspace_name: Option<String>,
  pub icon: Option<String>,
  pub member_count: i64,
}

impl TryFrom<AFWorkspaceWithMemberCountRow> for AFWorkspace {
  type Error = AppError;

  fn try_from(value: AFWorkspaceWithMemberCountRow) -> Result<Self, Self::Error> {
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
      owner_email: value.owner_email.unwrap_or_default(),
      workspace_type: value.workspace_type,
      workspace_name,
      created_at,
      icon,
      member_count: Some(value.member_count),
      role: None,
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
  pub avatar_url: Option<String>,
  pub role: AFRole,
  pub created_at: Option<DateTime<Utc>>,
}

impl From<AFWorkspaceMemberRow> for AFWorkspaceMember {
  fn from(value: AFWorkspaceMemberRow) -> Self {
    AFWorkspaceMember {
      name: value.name.clone(),
      email: value.email.clone(),
      role: value.role.clone(),
      avatar_url: value.avatar_url.clone(),
      joined_at: value.created_at,
    }
  }
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

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
#[repr(i16)]
pub enum AFBlobStatus {
  Ok = 0,
  PolicyViolation = 1,
  Failed = 2,
  Pending = 3,
}

impl From<i16> for AFBlobStatus {
  fn from(value: i16) -> Self {
    match value {
      0 => AFBlobStatus::Ok,
      1 => AFBlobStatus::PolicyViolation,
      2 => AFBlobStatus::Failed,
      3 => AFBlobStatus::Pending,
      _ => AFBlobStatus::Ok,
    }
  }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
#[repr(i16)]
pub enum AFBlobSource {
  UserUpload = 0,
  AIGen = 1,
}

impl From<i16> for AFBlobSource {
  fn from(value: i16) -> Self {
    match value {
      0 => AFBlobSource::UserUpload,
      1 => AFBlobSource::AIGen,
      _ => AFBlobSource::UserUpload,
    }
  }
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct AFBlobMetadataRow {
  pub workspace_id: Uuid,
  pub file_id: String,
  pub file_type: String,
  pub file_size: i64,
  pub modified_at: DateTime<Utc>,
  #[serde(default)]
  pub status: i16,
  #[serde(default)]
  pub source: i16,
  #[serde(default)]
  pub source_metadata: serde_json::Value,
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
  pub owner_uid: i64,
  pub deleted_at: Option<DateTime<Utc>>,
  pub created_at: Option<DateTime<Utc>>,
  pub updated_at: DateTime<Utc>,
}

#[derive(FromRow, Clone, Debug)]
pub struct AFCollabData {
  pub oid: Uuid,
  pub partition_key: i32,
  pub updated_at: DateTime<Utc>,
  pub blob: Vec<u8>,
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

#[derive(sqlx::Type, Serialize, Debug)]
pub struct AFWebUserWithEmailColumn {
  uuid: Uuid,
  name: String,
  email: String,
  avatar_url: Option<String>,
}

fn mask_web_user_email(email: &str) -> String {
  email
    .split('@')
    .next()
    .map(|part| part.chars().take(6).collect())
    .unwrap_or_default()
}

impl From<AFWebUserWithEmailColumn> for AFWebUserWithObfuscatedName {
  fn from(val: AFWebUserWithEmailColumn) -> Self {
    let obfuscated_name = if val.name == val.email {
      mask_web_user_email(&val.email)
    } else {
      val.name.clone()
    };

    AFWebUserWithObfuscatedName {
      uuid: val.uuid,
      name: obfuscated_name,
      avatar_url: val.avatar_url,
    }
  }
}

pub struct AFGlobalCommentRow {
  pub user: Option<AFWebUserWithEmailColumn>,
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
  pub react_users: Vec<AFWebUserWithEmailColumn>,
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

#[derive(Debug, FromRow, Serialize, sqlx::Type)]
pub struct AFTemplateCategoryRow {
  pub category_id: Uuid,
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
      id: value.category_id,
      name: value.name,
      icon: value.icon,
      bg_color: value.bg_color,
      description: value.description,
      category_type: value.category_type.into(),
      priority: value.priority,
    }
  }
}

#[derive(Debug, FromRow, Serialize, sqlx::Type)]
#[sqlx(type_name = "template_category_minimal_type")]
pub struct AFTemplateCategoryMinimalRow {
  pub category_id: Uuid,
  pub name: String,
  pub icon: String,
  pub bg_color: String,
}

impl From<AFTemplateCategoryMinimalRow> for TemplateCategoryMinimal {
  fn from(value: AFTemplateCategoryMinimalRow) -> Self {
    Self {
      id: value.category_id,
      name: value.name,
      icon: value.icon,
      bg_color: value.bg_color,
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
#[sqlx(type_name = "account_link_type")]
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

#[derive(Debug, Serialize, sqlx::Type)]
#[sqlx(type_name = "template_creator_type")]
pub struct AFTemplateCreatorRow {
  pub id: Uuid,
  pub name: String,
  pub avatar_url: String,
  pub account_links: Option<Vec<AccountLinkColumn>>,
  pub number_of_templates: i32,
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
      number_of_templates: value.number_of_templates,
    }
  }
}

#[derive(Debug, Serialize, sqlx::Type)]
#[sqlx(type_name = "template_creator_minimal_type")]
pub struct AFTemplateCreatorMinimalColumn {
  pub creator_id: Uuid,
  pub name: String,
  pub avatar_url: String,
}

impl From<AFTemplateCreatorMinimalColumn> for TemplateCreatorMinimal {
  fn from(value: AFTemplateCreatorMinimalColumn) -> Self {
    Self {
      id: value.creator_id,
      name: value.name,
      avatar_url: value.avatar_url,
    }
  }
}

#[derive(Debug, Serialize, FromRow, sqlx::Type)]
#[sqlx(type_name = "template_minimal_type")]
pub struct AFTemplateMinimalRow {
  pub view_id: Uuid,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
  pub name: String,
  pub description: String,
  pub view_url: String,
  pub creator: AFTemplateCreatorMinimalColumn,
  pub categories: Vec<AFTemplateCategoryMinimalRow>,
  pub is_new_template: bool,
  pub is_featured: bool,
}

impl From<AFTemplateMinimalRow> for TemplateMinimal {
  fn from(value: AFTemplateMinimalRow) -> Self {
    Self {
      view_id: value.view_id,
      created_at: value.created_at,
      last_updated_at: value.updated_at,
      name: value.name,
      description: value.description,
      creator: value.creator.into(),
      categories: value.categories.into_iter().map(|x| x.into()).collect(),
      view_url: value.view_url,
      is_new_template: value.is_new_template,
      is_featured: value.is_featured,
    }
  }
}

#[derive(Debug, Serialize, sqlx::Type)]
pub struct AFTemplateRow {
  pub view_id: Uuid,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
  pub name: String,
  pub description: String,
  pub about: String,
  pub view_url: String,
  pub creator: AFTemplateCreatorRow,
  pub categories: Vec<AFTemplateCategoryRow>,
  pub related_templates: Vec<AFTemplateMinimalRow>,
  pub is_new_template: bool,
  pub is_featured: bool,
}

impl From<AFTemplateRow> for Template {
  fn from(value: AFTemplateRow) -> Self {
    let mut related_templates: Vec<TemplateMinimal> = value
      .related_templates
      .into_iter()
      .map(|v| v.into())
      .collect();
    related_templates.sort_by_key(|t| t.created_at);
    related_templates.reverse();

    Self {
      view_id: value.view_id,
      created_at: value.created_at,
      last_updated_at: value.updated_at,
      name: value.name,
      description: value.description,
      about: value.about,
      view_url: value.view_url,
      creator: value.creator.into(),
      categories: value.categories.into_iter().map(|v| v.into()).collect(),
      related_templates,
      is_new_template: value.is_new_template,
      is_featured: value.is_featured,
    }
  }
}

#[derive(Debug, Serialize, sqlx::Type)]
pub struct AFTemplateGroupRow {
  pub category: AFTemplateCategoryMinimalRow,
  pub templates: Vec<AFTemplateMinimalRow>,
}

impl From<AFTemplateGroupRow> for TemplateGroup {
  fn from(value: AFTemplateGroupRow) -> Self {
    let mut templates: Vec<TemplateMinimal> =
      value.templates.into_iter().map(|v| v.into()).collect();
    templates.sort_by_key(|t| t.created_at);
    templates.reverse();
    Self {
      category: value.category.into(),
      templates,
    }
  }
}

#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct AFImportTask {
  pub task_id: Uuid,
  pub file_size: i64,
  pub workspace_id: String,
  pub created_by: i64,
  pub status: i16,
  pub metadata: serde_json::Value,
  pub created_at: DateTime<Utc>,
  #[serde(default)]
  pub file_url: Option<String>,
}
#[derive(sqlx::Type, Serialize, Deserialize, Debug)]
#[repr(i32)]
pub enum AFAccessRequestStatusColumn {
  Pending = 0,
  Approved = 1,
  Rejected = 2,
}

impl From<AFAccessRequestStatusColumn> for AccessRequestStatus {
  fn from(value: AFAccessRequestStatusColumn) -> Self {
    match value {
      AFAccessRequestStatusColumn::Pending => AccessRequestStatus::Pending,
      AFAccessRequestStatusColumn::Approved => AccessRequestStatus::Approved,
      AFAccessRequestStatusColumn::Rejected => AccessRequestStatus::Rejected,
    }
  }
}

#[derive(sqlx::Type, Serialize, Debug)]
pub struct AFAccessRequesterColumn {
  pub uid: i64,
  pub uuid: Uuid,
  pub name: String,
  pub email: String,
  pub avatar_url: Option<String>,
}

impl From<AFAccessRequesterColumn> for AccessRequesterInfo {
  fn from(value: AFAccessRequesterColumn) -> Self {
    Self {
      uid: value.uid,
      uuid: value.uuid,
      name: value.name,
      email: value.email,
      avatar_url: value.avatar_url,
    }
  }
}

#[derive(sqlx::Type, Serialize, Debug)]
pub struct AFAccessRequestMinimalColumn {
  pub request_id: Uuid,
  pub workspace_id: Uuid,
  pub requester_id: Uuid,
  pub view_id: Uuid,
}

impl From<AFAccessRequestMinimalColumn> for AccessRequestMinimal {
  fn from(value: AFAccessRequestMinimalColumn) -> Self {
    Self {
      request_id: value.request_id,
      workspace_id: value.workspace_id,
      requester_id: value.requester_id,
      view_id: value.view_id,
    }
  }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AFAccessRequestWithViewIdColumn {
  pub request_id: Uuid,
  pub workspace: AFWorkspaceWithMemberCountRow,
  pub requester: AccessRequesterInfo,
  pub view_id: Uuid,
  pub status: AFAccessRequestStatusColumn,
  pub created_at: DateTime<Utc>,
}

impl TryFrom<AFAccessRequestWithViewIdColumn> for AccessRequestWithViewId {
  type Error = anyhow::Error;

  fn try_from(value: AFAccessRequestWithViewIdColumn) -> Result<Self, Self::Error> {
    Ok(Self {
      request_id: value.request_id,
      workspace: value.workspace.try_into()?,
      requester: value.requester,
      view_id: value.view_id,
      status: value.status.into(),
      created_at: value.created_at,
    })
  }
}

#[derive(FromRow, Serialize, Debug)]
pub struct AFQuickNoteRow {
  pub quick_note_id: Uuid,
  pub data: serde_json::Value,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
}

impl From<AFQuickNoteRow> for QuickNote {
  fn from(value: AFQuickNoteRow) -> Self {
    Self {
      id: value.quick_note_id,
      data: value.data,
      created_at: value.created_at,
      last_updated_at: value.updated_at,
    }
  }
}

pub struct AFPublishViewWithPublishInfo {
  pub view_id: Uuid,
  pub publish_name: String,
  pub publisher_email: String,
  pub publish_timestamp: DateTime<Utc>,
  pub comments_enabled: bool,
  pub duplicate_enabled: bool,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_mask_web_user_email() {
    let name = "";
    let masked = mask_web_user_email(name);
    assert_eq!(masked, "");

    let name = "john@domain.com";
    let masked = mask_web_user_email(name);
    assert_eq!(masked, "john");

    let name = "jonathan@domain.com";
    let masked = mask_web_user_email(name);
    assert_eq!(masked, "jonath");
  }
}
