use crate::state::RedisClient;
use collab::core::collab_plugin::EncodedCollab;
use redis::AsyncCommands;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, trace};

#[derive(Clone)]
pub struct CollabMemCache {
  redis_client: Arc<Mutex<RedisClient>>,
  hits: Arc<AtomicU64>,
  total_attempts: Arc<AtomicU64>,
}

impl CollabMemCache {
  pub fn new(redis_client: RedisClient) -> Self {
    Self {
      redis_client: Arc::new(Mutex::new(redis_client)),
      hits: Arc::new(AtomicU64::new(0)),
      total_attempts: Arc::new(AtomicU64::new(0)),
    }
  }

  pub async fn get_encoded_collab(&self, object_id: &str) -> Option<EncodedCollab> {
    self.total_attempts.fetch_add(1, Ordering::Relaxed);
    let result = self
      .redis_client
      .lock()
      .await
      .get::<_, Option<Vec<u8>>>(object_id)
      .await;

    match result {
      Ok(Some(bytes)) => match EncodedCollab::decode_from_bytes(&bytes) {
        Ok(encoded_collab) => {
          self.hits.fetch_add(1, Ordering::Relaxed);
          Some(encoded_collab)
        },
        Err(err) => {
          error!("Failed to decode collab from redis cache bytes: {:?}", err);
          None
        },
      },
      Ok(None) => {
        trace!(
          "No encoded collab found in cache for object_id: {}",
          object_id
        );

        None
      },
      Err(err) => {
        error!("Failed to get encoded collab from redis: {:?}", err);
        None
      },
    }
  }

  pub async fn cache_encoded_collab(&self, object_id: String, encoded_collab: &EncodedCollab) {
    match encoded_collab.encode_to_bytes() {
      Ok(bytes) => {
        if let Err(err) = self.set_bytes_in_redis(object_id, bytes).await {
          error!("Failed to cache encoded collab: {:?}", err);
        }
      },
      Err(e) => {
        error!("Failed to encode collab to bytes: {:?}", e);
      },
    }
  }

  pub async fn cache_encoded_collab_bytes(&self, object_id: String, bytes: Vec<u8>) {
    if let Err(err) = self.set_bytes_in_redis(object_id, bytes).await {
      error!("Failed to cache encoded collab bytes: {:?}", err);
    }
  }

  // Helper function to set bytes in Redis.
  async fn set_bytes_in_redis(&self, object_id: String, bytes: Vec<u8>) -> redis::RedisResult<()> {
    self
      .redis_client
      .lock()
      .await
      .set::<_, Vec<u8>, ()>(object_id, bytes)
      .await
  }

  pub fn get_hit_rate(&self) -> f64 {
    let hits = self.hits.load(Ordering::Relaxed) as f64;
    let total_attempts = self.total_attempts.load(Ordering::Relaxed) as f64;

    if total_attempts == 0.0 {
      0.0
    } else {
      hits / total_attempts
    }
  }
}
