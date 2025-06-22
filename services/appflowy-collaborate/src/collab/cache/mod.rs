mod collab_cache;
pub mod disk_cache;
pub mod mem_cache;

use app_error::AppError;
use collab::entity::EncodedCollab;
pub use collab_cache::CollabCache;
use infra::thread_pool::ThreadPoolNoAbort;
use std::sync::Arc;
/// Threshold for spawning blocking tasks for decoding operations.
/// Data smaller than this will be processed on the current thread for efficiency.
/// Data larger than this will be spawned to avoid blocking the current thread.
pub const DECODE_SPAWN_THRESHOLD: usize = 4096; // 4KB

#[inline]
pub(crate) fn encode_collab_from_bytes(bytes: Vec<u8>) -> Result<EncodedCollab, AppError> {
  match EncodedCollab::decode_from_bytes(&bytes) {
    Ok(encoded_collab) => Ok(encoded_collab),
    Err(err) => Err(AppError::Internal(anyhow::anyhow!(
      "Failed to decode collab from bytes: {:?}",
      err
    ))),
  }
}

#[inline]
pub(crate) async fn encode_collab_from_bytes_with_thread_pool(
  thread_pool: &Arc<ThreadPoolNoAbort>,
  bytes: Vec<u8>,
) -> Result<EncodedCollab, AppError> {
  if bytes.len() <= DECODE_SPAWN_THRESHOLD {
    // For small data, decode on current thread for efficiency
    match EncodedCollab::decode_from_bytes(&bytes) {
      Ok(encoded_collab) => Ok(encoded_collab),
      Err(err) => Err(AppError::Internal(anyhow::anyhow!(
        "Failed to decode collab from bytes: {:?}",
        err
      ))),
    }
  } else {
    thread_pool
      .install(|| match EncodedCollab::decode_from_bytes(&bytes) {
        Ok(encoded_collab) => Ok(encoded_collab),
        Err(err) => Err(AppError::Internal(anyhow::anyhow!(
          "Failed to decode collab from bytes: {:?}",
          err
        ))),
      })
      .map_err(|err| {
        AppError::Internal(anyhow::anyhow!("Failed to spawn blocking task: {:?}", err))
      })?
  }
}
