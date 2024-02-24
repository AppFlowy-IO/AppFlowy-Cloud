use crate::biz::casbin::enforcer::{AFEnforcerCache, ActionCacheKey, PolicyCacheKey};
use crate::state::RedisClient;
use redis::AsyncCommands;

use anyhow::anyhow;
use app_error::AppError;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::error;

/// Expire time for cache in seconds. When the cache is expired, the enforcer will re-evaluate the policy.
const EXPIRE_TIME: u64 = 60 * 60 * 3;

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

#[async_trait]
impl AFEnforcerCache for AFEnforcerCacheImpl {
  async fn set_enforcer_result(&self, key: &PolicyCacheKey, value: bool) -> Result<(), AppError> {
    self
      .redis_client
      .lock()
      .await
      .set_ex::<&str, bool, ()>(key, value, EXPIRE_TIME)
      .await
      .map_err(|e| AppError::Internal(anyhow!("Failed to set enforcer result in redis: {}", e)))
  }

  async fn get_enforcer_result(&self, key: &PolicyCacheKey) -> Option<bool> {
    self
      .redis_client
      .lock()
      .await
      .get::<&str, Option<bool>>(key.as_ref())
      .await
      .ok()?
  }

  async fn remove_enforcer_result(&self, key: &PolicyCacheKey) {
    if let Err(err) = self
      .redis_client
      .lock()
      .await
      .del::<&str, ()>(key.as_ref())
      .await
    {
      error!("Failed to remove enforcer result from redis: {}", err);
    }
  }

  async fn set_action(&self, key: &ActionCacheKey, value: String) -> Result<(), AppError> {
    self
      .redis_client
      .lock()
      .await
      .set_ex::<&str, String, ()>(key, value, EXPIRE_TIME)
      .await
      .map_err(|e| AppError::Internal(anyhow!("Failed to set action in redis: {}", e)))
  }

  async fn get_action(&self, key: &ActionCacheKey) -> Option<String> {
    self
      .redis_client
      .lock()
      .await
      .get::<&str, Option<String>>(key.as_ref())
      .await
      .ok()?
  }

  async fn remove_action(&self, key: &ActionCacheKey) {
    if let Err(err) = self.redis_client.lock().await.del::<&str, ()>(key).await {
      error!("Failed to remove action from cache: {}", err);
    }
  }
}
