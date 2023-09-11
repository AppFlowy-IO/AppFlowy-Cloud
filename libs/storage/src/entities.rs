use serde::{Deserialize, Serialize};
use sqlx::types::{
  chrono::{DateTime, Utc},
  uuid::Uuid,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCollabParams {
  pub object_id: String,
  pub raw_data: Vec<u8>,
  pub len: usize,
  pub workspace_id: String,
}

impl CreateCollabParams {
  pub fn from_raw_data(object_id: &str, raw_data: Vec<u8>, workspace_id: &str) -> Self {
    let len = raw_data.len();
    Self {
      object_id: object_id.to_string(),
      raw_data,
      len,
      workspace_id: workspace_id.to_string(),
    }
  }
}

#[derive(Debug, sqlx::FromRow)]
pub struct AfWorkspace {
  pub workspace_id: Uuid,
  pub database_storage_id: Option<Uuid>,
  pub owner_uid: Option<Uuid>,
  pub created_at: Option<DateTime<Utc>>,
  pub workspace_type: i32,
  pub deleted_at: Option<DateTime<Utc>>,
  pub workspace_name: Option<String>,
}
