use crate::state::RedisClient;

use collab::core::collab_plugin::EncodedCollab;

use redis::AsyncCommands;

use rand::{thread_rng, Rng};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, trace};

const ENCODED_COLLAB_KEY_PREFIX: &str = "encoded_collab";

#[derive(Clone)]
pub struct CollabMemCache {
  /// Workaround for Redis cache
  ///   This cache instance has a unique identifier. Use only the current ID with the cache to ensure its relevance.
  ///   Using this ID to avoid outdated cache data after server restarts.
  id: u32,
  redis_client: Arc<Mutex<RedisClient>>,
}

impl CollabMemCache {
  pub fn new(redis_client: RedisClient) -> Self {
    let mut rng = thread_rng();
    let id: u32 = rng.gen();

    Self {
      id,
      redis_client: Arc::new(Mutex::new(redis_client)),
    }
  }

  pub async fn get_encoded_collab(&self, object_id: &str) -> Option<EncodedCollab> {
    let key = encoded_collab_key(self.id, object_id);
    let result = self
      .redis_client
      .lock()
      .await
      .get::<_, Option<Vec<u8>>>(&key)
      .await;
    match result {
      Ok(Some(bytes)) => match EncodedCollab::decode_from_bytes(&bytes) {
        Ok(encoded_collab) => Some(encoded_collab),
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

  pub async fn cache_encoded_collab(&self, object_id: &str, encoded_collab: &EncodedCollab) {
    match encoded_collab.encode_to_bytes() {
      Ok(bytes) => {
        self.cache_encoded_collab_bytes(object_id, bytes).await;
      },
      Err(e) => {
        error!("Failed to encode collab to bytes: {:?}", e);
      },
    }
  }

  pub async fn cache_encoded_collab_bytes(&self, object_id: &str, bytes: Vec<u8>) {
    let key = encoded_collab_key(self.id, object_id);
    if let Err(err) = self
      .redis_client
      .lock()
      .await
      .set::<_, Vec<u8>, ()>(&key, bytes)
      .await
    {
      error!("Failed to cache encoded collab: {:?}", err);
    }
  }
}

#[inline]
fn encoded_collab_key(id: u32, object_id: &str) -> String {
  format!("{}:{}:{}", id, ENCODED_COLLAB_KEY_PREFIX, object_id)
}
