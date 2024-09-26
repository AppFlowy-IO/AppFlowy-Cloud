use database_entity::dto::{AFWorkspace, AccessRequestStatus, AccessRequesterInfo};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::workspace_dto::{ViewIcon, ViewLayout};

#[derive(Serialize, Deserialize, Debug)]
pub struct AccessRequestView {
  pub view_id: String,
  pub name: String,
  pub icon: Option<ViewIcon>,
  pub layout: ViewLayout,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AccessRequest {
  pub request_id: Uuid,
  pub workspace: AFWorkspace,
  pub requester: AccessRequesterInfo,
  pub view: AccessRequestView,
  pub status: AccessRequestStatus,
  pub created_at: chrono::DateTime<chrono::Utc>,
}
