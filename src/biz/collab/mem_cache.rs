use crate::state::RedisClient;
use collab::core::collab_plugin::EncodedCollab;
use redis::AsyncCommands;

use anyhow::anyhow;
use app_error::AppError;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, instrument, trace};

#[derive(Clone)]
pub struct CollabMemCache {
  redis_client: Arc<Mutex<RedisClient>>,
}

impl CollabMemCache {
  pub fn new(redis_client: RedisClient) -> Self {
    Self {
      redis_client: Arc::new(Mutex::new(redis_client)),
    }
  }

  pub async fn remove_encode_collab(&self, object_id: &str) -> Result<(), AppError> {
    self
      .redis_client
      .lock()
      .await
      .del::<&str, ()>(object_id)
      .await
      .map_err(|err| {
        AppError::Internal(anyhow!(
          "Failed to remove encoded collab from redis: {:?}",
          err
        ))
      })
  }

  pub async fn get_encode_collab_bytes(&self, object_id: &str) -> Option<Vec<u8>> {
    let result = self
      .redis_client
      .lock()
      .await
      .get::<_, Option<Vec<u8>>>(object_id)
      .await;
    match result {
      Ok(bytes) => bytes,
      Err(err) => {
        error!("Failed to get encoded collab from redis: {:?}", err);
        None
      },
    }
  }

  #[instrument(level = "trace", skip_all)]
  pub async fn get_encode_collab_from_mem(&self, object_id: &str) -> Option<EncodedCollab> {
    match self.get_encode_collab_bytes(object_id).await {
      Some(bytes) => match EncodedCollab::decode_from_bytes(&bytes) {
        Ok(encoded_collab) => Some(encoded_collab),
        Err(err) => {
          error!("Failed to decode collab from redis cache bytes: {:?}", err);
          None
        },
      },
      None => {
        trace!(
          "No encoded collab found in cache for object_id: {}",
          object_id
        );
        None
      },
    }
  }

  #[instrument(level = "trace", skip_all, fields(object_id=%object_id))]
  pub async fn insert_encode_collab(&self, object_id: String, encoded_collab: EncodedCollab) {
    trace!("Inserting encode collab into cache: {}", object_id);
    let result = tokio::task::spawn_blocking(move || encoded_collab.encode_to_bytes()).await;
    match result {
      Ok(Ok(bytes)) => {
        if let Err(err) = self.set_bytes_in_redis(object_id, bytes).await {
          error!("Failed to cache encoded collab: {:?}", err);
        }
      },
      Ok(Err(err)) => {
        error!("Failed to encode collab to bytes: {:?}", err);
      },
      Err(e) => {
        error!("Failed to encode collab to bytes: {:?}", e);
      },
    }
  }

  pub async fn insert_encode_collab_bytes(&self, object_id: String, bytes: Vec<u8>) {
    if let Err(err) = self.set_bytes_in_redis(object_id, bytes).await {
      error!("Failed to cache encoded collab bytes: {:?}", err);
    }
  }

  /// Set bytes in redis with a 3 days expiration.
  async fn set_bytes_in_redis(&self, object_id: String, bytes: Vec<u8>) -> redis::RedisResult<()> {
    self
      .redis_client
      .lock()
      .await
      .set_ex::<_, Vec<u8>, ()>(object_id, bytes, 259200)
      .await
  }
}
