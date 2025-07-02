use chrono::{DateTime, Utc};
use database_entity::dto::{AFAccessLevel, AFRole};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::workspace_dto::{ViewIcon, ViewLayout};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedView {
  pub view_id: Uuid,
  pub access_level: AFAccessLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedUser {
  pub view_id: Uuid,
  pub email: String,
  pub name: String,
  pub access_level: AFAccessLevel,
  pub role: AFRole,
  pub avatar_url: Option<String>,
  pub pending_invitation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedViewDetails {
  pub view_id: Uuid,
  pub shared_with: Vec<SharedUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedViewDetailsRequest {
  pub ancestor_view_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedViews {
  pub shared_views: Vec<SharedView>,
  pub view_id_with_no_access: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokedAccess {
  pub email: String,
  pub view_id: Uuid,
}

#[derive(Clone)]
pub struct GuestInviteCode {
  pub code: String,
  pub email: String,
}

pub struct SharedFolderView {
  pub view_id: String,
  pub name: String,
  pub icon: Option<ViewIcon>,
  pub is_published: bool,
  pub layout: ViewLayout,
  pub created_at: DateTime<Utc>,
  pub last_edited_time: DateTime<Utc>,
  pub is_locked: Option<bool>,
  /// contains fields like `is_space`, and font information
  pub extra: Option<serde_json::Value>,
  pub children: Vec<SharedFolderView>,
  pub permission: AFAccessLevel,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ShareViewWithGuestRequest {
  pub view_id: Uuid,
  pub emails: Vec<String>,
  pub access_level: AFAccessLevel,
  // If false, the guest will need to accept the invitation before being added officially.
  // to the workspace.
  #[serde(default)]
  pub auto_confirm: bool,
}

#[derive(Serialize, Deserialize)]
pub struct RevokeSharedViewAccessRequest {
  pub emails: Vec<String>,
}
