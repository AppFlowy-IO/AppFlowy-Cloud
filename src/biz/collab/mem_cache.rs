use crate::state::RedisClient;
use bytes::Bytes;
use collab::core::collab_plugin::EncodedCollab;
use moka::future::Cache;
use moka::notification::RemovalCause;
use moka::policy::EvictionPolicy;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

#[derive(Clone)]
pub struct CollabMemCache {
  cache: Arc<Cache<String, Bytes>>,
  #[allow(dead_code)]
  redis_client: Arc<Mutex<RedisClient>>,
}

impl CollabMemCache {
  pub fn new(redis_client: RedisClient) -> Self {
    let eviction_listener = |key, value: Bytes, cause| {
      if matches!(cause, RemovalCause::Expired | RemovalCause::Size) {
        info!(
          "Evicted key {}. value:{}, cause:{:?}",
          key,
          value.len(),
          cause
        );
      }
    };

    let cache = Cache::builder()
        .weigher(|_key, value: &Bytes| -> u32 {
          value.len() as u32
        })
        // This cache will hold up to 200MiB of values.
        .max_capacity(200 * 1024 * 1024)
        .eviction_policy(EvictionPolicy::tiny_lfu())
        .eviction_listener(eviction_listener)
        .build();
    Self {
      cache: Arc::new(cache),
      redis_client: Arc::new(Mutex::new(redis_client)),
    }
  }

  pub async fn len(&self) -> usize {
    self.cache.entry_count() as usize
  }

  pub async fn get_encoded_collab(&self, object_id: &str) -> Option<EncodedCollab> {
    let bytes = self.cache.get(object_id).await?;
    tokio::task::spawn_blocking(move || match EncodedCollab::decode_from_bytes(&bytes) {
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
    let cache = self.cache.clone();
    tokio::task::spawn_blocking(move || match encoded_collab.encode_to_bytes() {
      Ok(bytes) => {
        tokio::spawn(async move { cache.insert(object_id, Bytes::from(bytes)).await });
      },
      Err(e) => {
        error!("Failed to encode collab to bytes: {:?}", e);
      },
    });
  }

  pub async fn remove_encoded_collab(&self, object_id: &str) {
    self.cache.invalidate(object_id).await;
  }

  pub fn cache_encoded_collab_bytes(&self, object_id: String, bytes: Vec<u8>) {
    let cache = self.cache.clone();
    tokio::spawn(async move { cache.insert(object_id, Bytes::from(bytes)).await });
  }
}
