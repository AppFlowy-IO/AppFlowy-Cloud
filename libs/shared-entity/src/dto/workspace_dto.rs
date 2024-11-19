use chrono::{DateTime, Utc};
use collab_entity::{CollabType, EncodedCollab};
use database_entity::dto::{AFRole, AFWebUser, AFWorkspaceInvitationStatus, PublishInfo};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::{collections::HashMap, ops::Deref};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Space {
  pub view_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
  pub view_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSpaceParams {
  pub space_permission: SpacePermission,
  pub name: String,
  pub space_icon: String,
  pub space_icon_color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePageParams {
  pub parent_view_id: String,
  pub layout: ViewLayout,
  pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePageParams {
  pub name: String,
  pub icon: Option<ViewIcon>,
  pub extra: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageCollabData {
  pub encoded_collab: Vec<u8>,
  pub row_data: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageCollab {
  pub view: FolderView,
  pub data: PageCollabData,
  pub owner: Option<AFWebUser>,
  pub last_editor: Option<AFWebUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedDuplicate {
  pub published_view_id: String,
  pub dest_view_id: String,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct RecentFolderView {
  #[serde(flatten)]
  pub view: FolderView,
  pub last_viewed_at: DateTime<Utc>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct FavoriteFolderView {
  #[serde(flatten)]
  pub view: FolderView,
  pub favorited_at: DateTime<Utc>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TrashFolderView {
  #[serde(flatten)]
  pub view: FolderView,
  pub deleted_at: DateTime<Utc>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct RecentSectionItems {
  pub views: Vec<RecentFolderView>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct FavoriteSectionItems {
  pub views: Vec<FavoriteFolderView>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TrashSectionItems {
  pub views: Vec<TrashFolderView>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct FolderView {
  pub view_id: String,
  pub name: String,
  pub icon: Option<ViewIcon>,
  pub is_space: bool,
  pub is_private: bool,
  pub is_published: bool,
  pub layout: ViewLayout,
  pub created_at: DateTime<Utc>,
  pub last_edited_time: DateTime<Utc>,
  /// contains fields like `is_space`, and font information
  pub extra: Option<serde_json::Value>,
  pub children: Vec<FolderView>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct FolderViewMinimal {
  pub view_id: String,
  pub name: String,
  pub icon: Option<ViewIcon>,
  pub layout: ViewLayout,
}

/// Publish info with actual view info
#[derive(Debug, Serialize, Deserialize)]
pub struct PublishInfoView {
  pub view: FolderViewMinimal,
  pub info: PublishInfo,
}

#[derive(Eq, PartialEq, Debug, Hash, Clone, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum IconType {
  Emoji = 0,
  Url = 1,
  Icon = 2,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct ViewIcon {
  pub ty: IconType,
  pub value: String,
}

#[derive(Eq, PartialEq, Debug, Hash, Clone, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum ViewLayout {
  Document = 0,
  Grid = 1,
  Board = 2,
  Calendar = 3,
  Chat = 4,
}

impl Default for ViewLayout {
  fn default() -> Self {
    Self::Document
  }
}

#[derive(Eq, PartialEq, Debug, Hash, Clone, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum SpacePermission {
  PublicToAll = 0,
  Private = 1,
}

#[derive(Default, Debug, Deserialize, Serialize)]
pub struct QueryWorkspaceParam {
  pub include_member_count: Option<bool>,
  pub include_role: Option<bool>,
}

#[derive(Default, Debug, Deserialize, Serialize)]
pub struct QueryWorkspaceFolder {
  pub depth: Option<u32>,
  pub root_view_id: Option<String>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct PublishedView {
  pub view_id: String,
  pub name: String,
  pub icon: Option<ViewIcon>,
  pub layout: ViewLayout,
  pub is_published: bool,
  /// contains fields like `is_space`, and font information
  pub extra: Option<serde_json::Value>,
  pub children: Vec<PublishedView>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AFDatabase {
  pub id: String,
  pub name: String,
  pub fields: Vec<AFDatabaseField>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AFDatabaseField {
  pub name: String,
  pub field_type: String,
}
