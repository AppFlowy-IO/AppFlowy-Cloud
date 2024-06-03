use crate::util::{validate_not_empty_payload, validate_not_empty_str};
use chrono::{DateTime, Utc};
use collab_entity::CollabType;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Display;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use tracing::error;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct CreateCollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,

  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,

  #[validate(custom = "validate_not_empty_payload")]
  pub encoded_collab_v1: Vec<u8>,

  pub collab_type: CollabType,
}

impl From<(String, CollabParams)> for CreateCollabParams {
  fn from((workspace_id, collab_params): (String, CollabParams)) -> Self {
    Self {
      workspace_id,
      object_id: collab_params.object_id,
      encoded_collab_v1: collab_params.encoded_collab_v1,
      collab_type: collab_params.collab_type,
    }
  }
}

impl CreateCollabParams {
  pub fn split(self) -> (CollabParams, String) {
    (
      CollabParams {
        object_id: self.object_id,
        encoded_collab_v1: self.encoded_collab_v1,
        collab_type: self.collab_type,
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
    }
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

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct InsertSnapshotParams {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  #[validate(custom = "validate_not_empty_payload")]
  pub encoded_collab_v1: Vec<u8>,
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
  pub collab_type: CollabType,
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

impl Display for QueryCollabParams {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "workspace_id: {}, object_id: {}, collab_type: {:?}",
      self.workspace_id, self.object_id, self.collab_type
    )
  }
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
impl QueryCollab {
  pub fn new<T: ToString>(object_id: T, collab_type: CollabType) -> Self {
    let object_id = object_id.to_string();
    Self {
      object_id,
      collab_type,
    }
  }
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

#[derive(Serialize, Deserialize)]
pub struct WorkspaceUsage {
  pub total_document_size: i64,
}

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

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct QueryWorkspaceMember {
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,

  pub uid: i64,
}

#[derive(Serialize, Deserialize)]
pub struct AFCollabMember {
  pub uid: i64,
  pub oid: String,
  pub permission: AFPermission,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone, Hash)]
#[repr(i32)]
pub enum AFRole {
  Owner = 1,
  Member = 2,
  Guest = 3,
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

impl From<&str> for AFRole {
  fn from(value: &str) -> Self {
    match i32::from_str(value) {
      Ok(value) => value.into(),
      Err(_) => AFRole::Guest,
    }
  }
}

impl From<AFRole> for i32 {
  fn from(role: AFRole) -> Self {
    role as i32
  }
}

impl From<&AFRole> for i32 {
  fn from(role: &AFRole) -> Self {
    role.clone() as i32
  }
}

impl PartialOrd for AFRole {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for AFRole {
  fn cmp(&self, other: &Self) -> Ordering {
    let left = i32::from(self);
    let right = i32::from(other);
    // lower value has higher priority
    left.cmp(&right).reverse()
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

impl From<&AFRole> for AFAccessLevel {
  fn from(value: &AFRole) -> Self {
    match value {
      AFRole::Owner => AFAccessLevel::FullAccess,
      AFRole::Member => AFAccessLevel::ReadAndWrite,
      AFRole::Guest => AFAccessLevel::ReadOnly,
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

impl From<&str> for AFAccessLevel {
  fn from(value: &str) -> Self {
    match i32::from_str(value) {
      Ok(value) => AFAccessLevel::from(value),
      Err(_) => AFAccessLevel::ReadOnly,
    }
  }
}

impl From<AFAccessLevel> for i32 {
  fn from(level: AFAccessLevel) -> Self {
    level as i32
  }
}

impl From<&AFAccessLevel> for i32 {
  fn from(level: &AFAccessLevel) -> Self {
    *level as i32
  }
}

impl PartialOrd for AFAccessLevel {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for AFAccessLevel {
  fn cmp(&self, other: &Self) -> Ordering {
    let left = i32::from(self);
    let right = i32::from(other);
    left.cmp(&right)
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

#[derive(Debug, Serialize, Deserialize)]
pub struct AFWorkspace {
  pub workspace_id: Uuid,
  pub database_storage_id: Uuid,
  pub owner_uid: i64,
  pub owner_name: String,
  pub workspace_type: i32,
  pub workspace_name: String,
  pub created_at: DateTime<Utc>,
  pub icon: String,
}

#[derive(Serialize, Deserialize)]
pub struct AFWorkspaces(pub Vec<AFWorkspace>);

#[derive(Serialize, Deserialize)]
pub struct AFUserWorkspaceInfo {
  pub user_profile: AFUserProfile,
  pub visiting_workspace: AFWorkspace,
  pub workspaces: Vec<AFWorkspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct AFWorkspaceMember {
  pub name: String,
  pub email: String,
  pub role: AFRole,
  pub avatar_url: Option<String>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct AFWorkspaceInvitation {
  pub invite_id: Uuid,
  pub workspace_id: Uuid,
  pub workspace_name: Option<String>,
  pub inviter_email: Option<String>,
  pub inviter_name: Option<String>,
  pub status: AFWorkspaceInvitationStatus,
  pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
#[repr(i16)]
pub enum AFWorkspaceInvitationStatus {
  Pending = 0,
  Accepted = 1,
  Rejected = 2,
}

impl From<i16> for AFWorkspaceInvitationStatus {
  fn from(value: i16) -> Self {
    match value {
      0 => AFWorkspaceInvitationStatus::Pending,
      1 => AFWorkspaceInvitationStatus::Accepted,
      2 => AFWorkspaceInvitationStatus::Rejected,
      _ => {
        error!("Invalid role id: {}", value);
        AFWorkspaceInvitationStatus::Pending
      },
    }
  }
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct CreateChatParams {
  #[validate(custom = "validate_not_empty_str")]
  pub chat_id: String,
  pub name: String,
  pub rag_ids: Vec<String>,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct UpdateChatParams {
  #[validate(custom = "validate_not_empty_str")]
  pub chat_id: String,

  #[validate(custom = "validate_not_empty_str")]
  pub name: Option<String>,

  pub rag_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct CreateChatMessageParams {
  #[validate(custom = "validate_not_empty_str")]
  pub content: String,
  pub message_type: ChatMessageType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChatMessageMetaParams {
  pub message_id: i64,
  pub meta_data: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChatMessageContentParams {
  pub chat_id: String,
  pub message_id: i64,
  pub content: String,
}

#[derive(Debug, Clone, Default, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum ChatMessageType {
  System = 0,
  #[default]
  User = 1,
}

impl CreateChatMessageParams {
  pub fn new_system<T: ToString>(content: T) -> Self {
    Self {
      content: content.to_string(),
      message_type: ChatMessageType::System,
    }
  }

  pub fn new_user<T: ToString>(content: T) -> Self {
    Self {
      content: content.to_string(),
      message_type: ChatMessageType::User,
    }
  }
}
#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct GetChatMessageParams {
  pub cursor: MessageCursor,
  pub limit: u64,
}

impl GetChatMessageParams {
  pub fn offset(offset: u64, limit: u64) -> Self {
    Self {
      cursor: MessageCursor::Offset(offset),
      limit,
    }
  }

  pub fn after_message_id(after_message_id: i64, limit: u64) -> Self {
    Self {
      cursor: MessageCursor::AfterMessageId(after_message_id),
      limit,
    }
  }
  pub fn before_message_id(before_message_id: i64, limit: u64) -> Self {
    Self {
      cursor: MessageCursor::BeforeMessageId(before_message_id),
      limit,
    }
  }

  pub fn next_back(limit: u64) -> Self {
    Self {
      cursor: MessageCursor::NextBack,
      limit,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageCursor {
  Offset(u64),
  AfterMessageId(i64),
  BeforeMessageId(i64),
  NextBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
  pub author: ChatAuthor,
  pub message_id: i64,
  pub content: String,
  pub created_at: DateTime<Utc>,
  pub meta_data: serde_json::Value,
  pub reply_message_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QAChatMessage {
  pub question: ChatMessage,
  pub answer: Option<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeatedChatMessage {
  pub messages: Vec<ChatMessage>,
  pub has_more: bool,
  pub total: i64,
}

#[derive(Debug, Default, Clone, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum ChatAuthorType {
  Unknown = 0,
  Human = 1,
  #[default]
  System = 2,
  AI = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatAuthor {
  pub author_id: i64,
  #[serde(default)]
  pub author_type: ChatAuthorType,
  #[serde(default)]
  #[serde(skip_serializing_if = "Option::is_none")]
  pub meta: Option<serde_json::Value>,
}

impl ChatAuthor {
  pub fn new(author_id: i64, author_type: ChatAuthorType) -> Self {
    Self {
      author_id,
      author_type,
      meta: None,
    }
  }

  pub fn ai() -> Self {
    Self {
      author_id: 0,
      author_type: ChatAuthorType::AI,
      meta: None,
    }
  }
}

#[derive(Debug, Clone)]
pub struct AFCollabEmbeddingParams {
  pub fragment_id: String,
  pub object_id: String,
  pub collab_type: CollabType,
  pub content_type: EmbeddingContentType,
  pub content: String,
  pub embedding: Option<Vec<f32>>,
}

/// Type of content stored by the embedding.
/// Currently only plain text of the document is supported.
/// In the future, we might support other kinds like i.e. PDF, images or image-extracted text.
#[repr(i32)]
#[derive(Debug, Copy, Clone, Serialize_repr, Deserialize_repr, Eq, PartialEq)]
pub enum EmbeddingContentType {
  /// The plain text representation of the document.
  PlainText = 0,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChatMessageResponse {
  pub answer: Option<ChatMessage>,
}
