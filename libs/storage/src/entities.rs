use collab_define::CollabType;
use serde::{Deserialize, Serialize};
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
