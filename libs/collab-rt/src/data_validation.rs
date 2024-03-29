use crate::error::RealtimeError;

use collab::core::collab::DocStateSource;
use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;

pub fn validate_encode_collab(
  object_id: &str,
  data: &[u8],
  collab_type: &CollabType,
) -> Result<(), RealtimeError> {
  let encoded_collab =
    EncodedCollab::decode_from_bytes(data).map_err(|err| RealtimeError::Internal(err.into()))?;
  let collab = Collab::new_with_doc_state(
    CollabOrigin::Empty,
    object_id,
    DocStateSource::FromDocState(encoded_collab.doc_state.to_vec()),
    vec![],
    false,
  )
  .map_err(|err| RealtimeError::Internal(err.into()))?;

  validate_collab(&collab, collab_type)
}

pub fn validate_collab(collab: &Collab, collab_type: &CollabType) -> Result<(), RealtimeError> {
  match collab_type {
    CollabType::Document => collab_document::document::Document::validate(collab)
      .map_err(|err| RealtimeError::NoRequiredCollabData(err.to_string()))?,
    CollabType::Database => {},
    CollabType::Folder => collab_folder::Folder::validate(collab)
      .map(|_| ())
      .map_err(|err| RealtimeError::NoRequiredCollabData(err.to_string()))?,
    CollabType::DatabaseRow => {},
    _ => {},
  }

  Ok(())
}
