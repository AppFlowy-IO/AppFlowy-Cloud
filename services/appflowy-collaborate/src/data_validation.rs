use crate::error::RealtimeError;
use async_trait::async_trait;

use collab::core::collab::DataSource;
use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use database_entity::dto::CollabParams;
use tracing::instrument;

#[async_trait]
pub trait CollabValidator {
  async fn check_encode_collab(&self) -> Result<(), RealtimeError>;
}

#[async_trait]
impl CollabValidator for CollabParams {
  async fn check_encode_collab(&self) -> Result<(), RealtimeError> {
    validate_encode_collab(&self.object_id, &self.encoded_collab_v1, &self.collab_type).await
  }
}

#[instrument(level = "trace", skip(data), fields(len = %data.len()))]
pub async fn validate_encode_collab(
  object_id: &str,
  data: &[u8],
  collab_type: &CollabType,
) -> Result<(), RealtimeError> {
  let collab_type = collab_type.clone();
  let object_id = object_id.to_string();
  let data = data.to_vec();

  tokio::task::spawn_blocking(move || {
    let encoded_collab =
      EncodedCollab::decode_from_bytes(&data).map_err(|err| RealtimeError::Internal(err.into()))?;
    let collab = Collab::new_with_source(
      CollabOrigin::Empty,
      &object_id,
      DataSource::DocStateV1(encoded_collab.doc_state.to_vec()),
      vec![],
      false,
    )
    .map_err(|err| RealtimeError::Internal(err.into()))?;

    collab_type
      .validate(&collab)
      .map_err(|err| RealtimeError::NoRequiredCollabData(err.to_string()))?;
    Ok::<(), RealtimeError>(())
  })
  .await?
}
