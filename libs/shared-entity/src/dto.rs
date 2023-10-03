// Data Transfer Objects (DTO)

use collab_define::CollabType;
use serde::Deserialize;
use serde::Serialize;
use validator::Validate;
use validator::ValidationError;

use gotrue_entity::AccessTokenResponse;

#[derive(Deserialize, Serialize)]
pub struct WorkspaceMembersParams {
  pub workspace_uuid: uuid::Uuid,
  pub member_emails: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub struct SignInParams {
  pub email: String,
  pub password: String,
}

#[derive(Default, Deserialize, Serialize)]
pub struct UserUpdateParams {
  pub name: Option<String>,
  pub email: Option<String>,
  pub password: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct UpdateUsernameParams {
  pub new_name: String,
}

impl UserUpdateParams {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_name<T: ToString>(mut self, name: T) -> Self {
    self.name = Some(name.to_string());
    self
  }

  pub fn with_email<T: ToString>(mut self, email: T) -> Self {
    self.email = Some(email.to_string());
    self
  }

  pub fn with_password(mut self, password: &str) -> Self {
    self.password = Some(password.to_owned());
    self
  }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInPasswordResponse {
  pub access_token_resp: AccessTokenResponse,
  pub is_new: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInTokenResponse {
  pub is_new: bool,
}

pub type RawData = Vec<u8>;

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct InsertCollabParams {
  pub object_id: String,
  #[validate(custom = "validate_not_empty_payload")]
  pub raw_data: Vec<u8>,
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
  pub collab_type: CollabType,
}

impl InsertCollabParams {
  pub fn new<T: ToString>(
    object_id: T,
    collab_type: CollabType,
    raw_data: Vec<u8>,
    workspace_id: String,
  ) -> Self {
    let object_id = object_id.to_string();
    Self {
      object_id,
      collab_type,
      raw_data,
      workspace_id,
    }
  }
}

impl InsertCollabParams {
  pub fn from_raw_data(
    object_id: &str,
    collab_type: CollabType,
    raw_data: Vec<u8>,
    workspace_id: &str,
  ) -> Self {
    let object_id = object_id.to_string();
    let workspace_id = workspace_id.to_string();
    Self {
      object_id,
      collab_type,
      raw_data,
      workspace_id,
    }
  }
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct InsertSnapshotParams {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  #[validate(custom = "validate_not_empty_payload")]
  pub raw_data: Vec<u8>,
  pub len: i32,
  #[validate(custom = "validate_not_empty_str")]
  pub workspace_id: String,
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct DeleteCollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
}

fn validate_not_empty_payload(payload: &[u8]) -> Result<(), ValidationError> {
  if payload.is_empty() {
    return Err(ValidationError::new("should not be empty payload"));
  }
  Ok(())
}

#[derive(Debug, Clone, Validate, Serialize, Deserialize)]
pub struct QueryCollabParams {
  #[validate(custom = "validate_not_empty_str")]
  pub object_id: String,
  pub collab_type: CollabType,
}

fn validate_not_empty_str(s: &str) -> Result<(), ValidationError> {
  if s.is_empty() {
    return Err(ValidationError::new("should not be empty string"));
  }
  Ok(())
}
