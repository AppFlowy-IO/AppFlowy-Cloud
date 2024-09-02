use anyhow::anyhow;
use app_error::AppError;
use collab::entity::EncodedCollab;

#[inline]
pub(crate) async fn encode_collab_from_bytes(bytes: Vec<u8>) -> Result<EncodedCollab, AppError> {
  // FIXME: Implement metrics to determine the appropriate data size for decoding in a blocking task.
  tokio::task::spawn_blocking(move || match EncodedCollab::decode_from_bytes(&bytes) {
    Ok(encoded_collab) => Ok(encoded_collab),
    Err(err) => Err(AppError::Internal(anyhow!(
      "Failed to decode collab from bytes: {:?}",
      err
    ))),
  })
  .await?
}
