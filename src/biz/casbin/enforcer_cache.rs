use crate::biz::casbin::enforcer::{AFEnforcerCache, ActionCacheKey, PolicyCacheKey};
use crate::state::RedisClient;
use redis::AsyncCommands;

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::error;

#[derive(Clone)]
pub struct AFEnforcerCacheImpl {
  redis_client: Arc<Mutex<RedisClient>>,
}

impl AFEnforcerCacheImpl {
  pub fn new(redis_client: RedisClient) -> Self {
    Self {
      redis_client: Arc::new(Mutex::new(redis_client)),
    }
  }
}

impl AFEnforcerCache for AFEnforcerCacheImpl {
  async fn set_enforcer_result(&self, key: &PolicyCacheKey, value: bool) {
    if let Err(err) = self
      .redis_client
      .lock()
      .await
      .set::<&str, bool, ()>(key, value)
      .await
    {
      error!("Failed to set enforcer result in cache: {}", err);
    }
  }

  async fn get_enforcer_result(&self, key: &PolicyCacheKey) -> Option<bool> {
    self.redis_client.lock().await.get(key.as_ref()).await.ok()
  }

  async fn remove_enforcer_result(&self, key: &PolicyCacheKey) {
    if let Err(err) = self
      .redis_client
      .lock()
      .await
      .del::<&str, ()>(key.as_ref())
      .await
    {
      error!("Failed to remove enforcer result from cache: {}", err);
    }
  }

  async fn set_action(&self, key: &ActionCacheKey, value: String) {
    if let Err(err) = self
      .redis_client
      .lock()
      .await
      .set::<&str, String, ()>(key, value)
      .await
    {
      error!("Failed to set action in cache: {}", err);
    }
  }

  async fn get_action(&self, key: &ActionCacheKey) -> Option<String> {
    self.redis_client.lock().await.get(key.as_ref()).await.ok()
  }

  async fn remove_action(&self, key: &ActionCacheKey) {
    if let Err(err) = self.redis_client.lock().await.del::<&str, ()>(key).await {
      error!("Failed to remove action from cache: {}", err);
    }
  }
}
