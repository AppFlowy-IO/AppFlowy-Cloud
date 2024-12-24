use app_error::AppError;
use async_trait::async_trait;
use collab_rt_protocol::validate_encode_collab;
use database_entity::dto::CollabParams;

#[async_trait]
pub trait CollabValidator {
  async fn check_encode_collab(&self) -> Result<(), AppError>;
}

#[async_trait]
impl CollabValidator for CollabParams {
  async fn check_encode_collab(&self) -> Result<(), AppError> {
    validate_encode_collab(&self.object_id, &self.encoded_collab_v1, &self.collab_type)
      .await
      .map_err(|err| AppError::NoRequiredData(err.to_string()))
  }
}
