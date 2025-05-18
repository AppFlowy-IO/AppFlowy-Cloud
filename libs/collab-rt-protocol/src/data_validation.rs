use anyhow::Error;
use collab::core::collab::{CollabOptions, DataSource};
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::Collab;
use collab_entity::CollabType;
use tracing::instrument;
use uuid::Uuid;

#[inline]
pub async fn collab_from_encode_collab(object_id: &Uuid, data: &[u8]) -> Result<Collab, Error> {
  let object_id = object_id.to_string();
  let data = data.to_vec();

  tokio::task::spawn_blocking(move || {
    let encoded_collab = EncodedCollab::decode_from_bytes(&data)?;
    let options = CollabOptions::new(object_id.to_string())
      .with_data_source(DataSource::DocStateV1(encoded_collab.doc_state.to_vec()));
    let collab = Collab::new_with_options(CollabOrigin::Empty, options)?;

    Ok::<_, Error>(collab)
  })
  .await?
}

#[instrument(level = "trace", skip(data), fields(len = %data.len()))]
#[inline]
pub async fn validate_encode_collab(
  object_id: &Uuid,
  data: &[u8],
  collab_type: &CollabType,
) -> Result<(), Error> {
  let collab = collab_from_encode_collab(object_id, data).await?;
  collab_type.validate_require_data(&collab)?;
  Ok::<(), Error>(())
}
