use crate::error::RealtimeError;

use collab::core::collab::DocStateSource;
use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use tracing::instrument;

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
    let collab = Collab::new_with_doc_state(
      CollabOrigin::Empty,
      &object_id,
      DocStateSource::FromDocState(encoded_collab.doc_state.to_vec()),
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
