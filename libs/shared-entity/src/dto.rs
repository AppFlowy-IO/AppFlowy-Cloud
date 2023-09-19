// Data Transfer Objects (DTO)

use sqlx::types::uuid;

#[derive(serde::Deserialize, serde::Serialize)]
pub struct WorkspaceMembers {
  pub workspace_uuid: uuid::Uuid,
  pub user_uids: Box<[i64]>,
}
