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

  pub async fn remove_encode_collab(&self, object_id: &str) {
    if let Err(err) = self
      .redis_client
      .lock()
      .await
      .del::<&str, ()>(object_id)
      .await
    {
      error!("Failed to remove encoded collab from redis: {:?}", err);
    }
  }

  pub async fn get_encode_collab_bytes(&self, object_id: &str) -> Option<Vec<u8>> {
    self.total_attempts.fetch_add(1, Ordering::Relaxed);
    let result = self
      .redis_client
      .lock()
      .await
      .get::<_, Option<Vec<u8>>>(object_id)
      .await;
    match result {
      Ok(bytes) => {
        self.hits.fetch_add(1, Ordering::Relaxed);
        bytes
      },
      Err(err) => {
        error!("Failed to get encoded collab from redis: {:?}", err);
        None
      },
    }
  }

  pub async fn get_encode_collab(&self, object_id: &str) -> Option<EncodedCollab> {
    match self.get_encode_collab_bytes(object_id).await {
      Some(bytes) => match EncodedCollab::decode_from_bytes(&bytes) {
        Ok(encoded_collab) => {
          self.hits.fetch_add(1, Ordering::Relaxed);
          Some(encoded_collab)
        },
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

  pub async fn insert_encode_collab(&self, object_id: String, encoded_collab: &EncodedCollab) {
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
