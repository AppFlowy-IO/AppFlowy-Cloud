use collab_define::CollabType;
use serde::{Deserialize, Serialize};
use sqlx::types::{
  chrono::{DateTime, Utc},
  uuid,
};
use validator::{Validate, ValidationError};

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct InsertCollabParams {
  pub uid: i64,
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  pub raw_data: Vec<u8>,
  pub len: i32,
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
  pub collab_type: CollabType,
}
fn validate_not_empty_str(s: &str) -> Result<(), ValidationError> {
  if s.is_empty() {
    return Err(ValidationError::new("should not be empty string"));
  }
  Ok(())
}

impl InsertCollabParams {
  pub fn from_raw_data(
    uid: i64,
    object_id: &str,
    collab_type: CollabType,
    raw_data: Vec<u8>,
    workspace_id: &str,
  ) -> Self {
    let len = raw_data.len() as i32;
    let object_id = object_id.to_string();
    let workspace_id = workspace_id.to_string();
    Self {
      uid,
      object_id,
      collab_type,
      raw_data,
      len,
      workspace_id,
    }
  }
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct QueryCollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  pub collab_type: CollabType,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct AfWorkspace {
  pub workspace_id: uuid::Uuid,
  pub database_storage_id: Option<sqlx::types::uuid::Uuid>,
  pub owner_uid: Option<i64>,
  pub created_at: Option<DateTime<Utc>>,
  pub workspace_type: i32,
  pub deleted_at: Option<DateTime<Utc>>,
  pub workspace_name: Option<String>,
}

#[derive(Debug, sqlx::FromRow, Deserialize, Serialize)]
pub struct AfUserProfileView {
  pub uid: Option<i64>,
  pub uuid: Option<uuid::Uuid>,
  pub email: Option<String>,
  pub password: Option<String>,
  pub name: Option<String>,
  pub encryption_sign: Option<String>,
  pub deleted_at: Option<DateTime<Utc>>,
  pub updated_at: Option<DateTime<Utc>>,
  pub created_at: Option<DateTime<Utc>>,
  pub latest_workspace_id: Option<uuid::Uuid>,
}

#[derive(Debug, sqlx::FromRow, Deserialize, Serialize)]
pub struct AfWorkspaces(pub Vec<AfWorkspace>);

impl AfWorkspaces {
  pub fn get_latest(&self, profile: AfUserProfileView) -> Option<AfWorkspace> {
    match profile.latest_workspace_id {
      Some(ws_id) => self.0.iter().find(|ws| ws.workspace_id == ws_id).cloned(),
      None => None,
    }
  }
}
