use serde::{Deserialize, Serialize};
use sqlx::types::{
  chrono::{DateTime, Utc},
  uuid,
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

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AfWorkspace {
  pub workspace_id: uuid::Uuid,
  pub database_storage_id: Option<sqlx::types::uuid::Uuid>,
  pub owner_uid: Option<i64>,
  pub created_at: Option<DateTime<Utc>>,
  pub workspace_type: i32,
  pub deleted_at: Option<DateTime<Utc>>,
  pub workspace_name: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct AfUserProfileView {
  pub uid: Option<i64>,         // Made this field nullable based on the error
  pub uuid: Option<uuid::Uuid>, // Made this field nullable based on the error
  pub email: Option<String>,    // Made this field nullable based on the error
  pub password: Option<String>, // Made this field nullable based on the error
  pub name: Option<String>,     // Made this field nullable based on the error
  pub encryption_sign: Option<String>, // Made this field nullable based on the error
  pub deleted_at: Option<DateTime<Utc>>,
  pub updated_at: Option<DateTime<Utc>>,
  pub created_at: Option<DateTime<Utc>>,
  pub latest_workspace_id: Option<uuid::Uuid>,
}

pub struct AfWorkspaces(pub Vec<AfWorkspace>);
impl AfWorkspaces {
  pub fn get_latest(&self, profile: AfUserProfileView) -> Option<AfWorkspace> {
    match profile.latest_workspace_id {
      Some(ws_id) => self.0.iter().find(|ws| ws.workspace_id == ws_id).cloned(),
      None => None,
    }
  }
}
