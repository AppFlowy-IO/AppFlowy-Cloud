use crate::state::RedisClient;
use collab::core::collab_plugin::EncodedCollab;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::error;

#[derive(Clone)]
pub struct CollabMemCache {
  lru_cache: Arc<Mutex<LruCache<String, Vec<u8>>>>,
}

impl CollabMemCache {
  pub fn new(_redis_client: RedisClient) -> Self {
    let lru = LruCache::new(NonZeroUsize::new(3000).unwrap());
    Self {
      lru_cache: Arc::new(Mutex::new(lru)),
    }
  }

  pub async fn len(&self) -> usize {
    self
      .lru_cache
      .try_lock()
      .map(|cache| cache.len())
      .unwrap_or(0)
  }

  pub async fn get_encoded_collab(&self, object_id: &str) -> Option<EncodedCollab> {
    let cache = self.lru_cache.lock().await.get(object_id)?.clone();
    tokio::task::spawn_blocking(move || match EncodedCollab::decode_from_bytes(&cache) {
      Ok(encoded_collab) => Some(encoded_collab),
      Err(err) => {
        error!("Failed to decode collab from redis cache bytes: {:?}", err);
        None
      },
    })
    .await
    .ok()?
  }

  pub fn cache_encoded_collab(&self, object_id: String, encoded_collab: &EncodedCollab) {
    let encoded_collab = encoded_collab.clone();
    let cache = self.lru_cache.clone();
    tokio::task::spawn_blocking(move || match encoded_collab.encode_to_bytes() {
      Ok(bytes) => {
        tokio::spawn(async move { cache.lock().await.put(object_id, bytes) });
      },
      Err(e) => {
        error!("Failed to encode collab to bytes: {:?}", e);
      },
    });
  }

  pub fn cache_encoded_collab_bytes(&self, object_id: String, bytes: Vec<u8>) {
    let cache = self.lru_cache.clone();
    tokio::spawn(async move { cache.lock().await.put(object_id, bytes) });
  }
}
