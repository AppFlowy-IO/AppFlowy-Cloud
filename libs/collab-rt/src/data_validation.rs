use crate::error::RealtimeError;

use collab::core::collab::DataSource;
use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use tracing::instrument;

#[instrument(level = "trace", skip(data), fields(len = %data.len()))]
pub fn validate_encode_collab(
  object_id: &str,
  data: &[u8],
  collab_type: &CollabType,
) -> Result<(), RealtimeError> {
  let encoded_collab =
    EncodedCollab::decode_from_bytes(data).map_err(|err| RealtimeError::Internal(err.into()))?;
  let collab = Collab::new_with_source(
    CollabOrigin::Empty,
    object_id,
    DataSource::DocStateV1(encoded_collab.doc_state.to_vec()),
    vec![],
    false,
  )
  .map_err(|err| RealtimeError::Internal(err.into()))?;

  collab_type
    .validate(&collab)
    .map_err(|err| RealtimeError::NoRequiredCollabData(err.to_string()))?;
  Ok(())
}
