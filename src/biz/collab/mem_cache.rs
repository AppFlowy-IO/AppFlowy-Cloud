use crate::state::RedisClient;

use collab::core::collab_plugin::EncodedCollab;
use database_entity::dto::QueryCollab;
use redis::AsyncCommands;

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::error;

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

  pub async fn get_encoded_collab(&self, query: &QueryCollab) -> Option<EncodedCollab> {
    let key = encoded_collab_key(&query.object_id);
    self
      .redis_client
      .lock()
      .await
      .get::<_, Vec<u8>>(&key)
      .await
      .ok()
      .and_then(|bytes| EncodedCollab::decode_from_bytes(&bytes).ok())
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
    let key = encoded_collab_key(object_id);
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
fn encoded_collab_key(object_id: &str) -> String {
  format!("encoded_collab:{}", object_id)
}
