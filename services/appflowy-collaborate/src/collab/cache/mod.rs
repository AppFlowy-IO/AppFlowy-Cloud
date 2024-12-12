mod collab_cache;
pub mod disk_cache;
pub mod mem_cache;

use app_error::AppError;
use collab::entity::EncodedCollab;
pub use collab_cache::CollabCache;

#[inline]
pub(crate) async fn encode_collab_from_bytes(bytes: Vec<u8>) -> Result<EncodedCollab, AppError> {
  // FIXME: Implement metrics to determine the appropriate data size for decoding in a blocking task.
  tokio::task::spawn_blocking(move || match EncodedCollab::decode_from_bytes(&bytes) {
    Ok(encoded_collab) => Ok(encoded_collab),
    Err(err) => Err(AppError::Internal(anyhow::anyhow!(
      "Failed to decode collab from bytes: {:?}",
      err
    ))),
  })
  .await
  .map_err(|err| AppError::Internal(anyhow::anyhow!("Failed to spawn blocking task: {:?}", err)))?
}
