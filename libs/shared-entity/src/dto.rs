// Data Transfer Objects (DTO)

use sqlx::types::uuid;

#[derive(serde::Deserialize, serde::Serialize)]
pub struct WorkspaceMembersParams {
  pub workspace_uuid: uuid::Uuid,
  pub member_emails: Vec<String>,
}
