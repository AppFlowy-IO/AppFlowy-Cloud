use anyhow::Error;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::Collab;
use collab_entity::CollabType;
use tracing::instrument;

#[instrument(level = "trace", skip(data), fields(len = %data.len()))]
pub async fn validate_encode_collab(
  object_id: &str,
  data: &[u8],
  collab_type: &CollabType,
) -> Result<(), Error> {
  let collab_type = collab_type.clone();
  let object_id = object_id.to_string();
  let data = data.to_vec();

  tokio::task::spawn_blocking(move || {
    let encoded_collab = EncodedCollab::decode_from_bytes(&data)?;
    let collab = Collab::new_with_source(
      CollabOrigin::Empty,
      &object_id,
      DataSource::DocStateV1(encoded_collab.doc_state.to_vec()),
      vec![],
      false,
    )?;

    collab_type.validate_require_data(&collab)?;
    Ok::<(), Error>(())
  })
  .await?
}
