use crate::biz::casbin::enforcer::{AFEnforcerCache, PolicyCacheKey};
use crate::state::RedisClient;
use redis::AsyncCommands;

use anyhow::anyhow;
use app_error::AppError;
use async_trait::async_trait;

use tracing::error;

/// Expire time for cache in seconds. When the cache is expired, the enforcer will re-evaluate the policy.
const EXPIRE_IN_ONE_DAY: u64 = 60 * 60 * 24;

#[derive(Clone)]
pub struct AFEnforcerCacheImpl {
  redis_client: RedisClient,
}

impl AFEnforcerCacheImpl {
  pub fn new(redis_client: RedisClient) -> Self {
    Self { redis_client }
  }
}

#[async_trait]
impl AFEnforcerCache for AFEnforcerCacheImpl {
  async fn set_enforcer_result(
    &mut self,
    key: &PolicyCacheKey,
    value: bool,
  ) -> Result<(), AppError> {
    self
      .redis_client
      .set_ex::<&str, bool, ()>(key, value, EXPIRE_IN_ONE_DAY)
      .await
      .map_err(|e| AppError::Internal(anyhow!("Failed to set enforcer result in redis: {}", e)))
  }

  async fn get_enforcer_result(&mut self, key: &PolicyCacheKey) -> Option<bool> {
    self
      .redis_client
      .get::<&str, Option<bool>>(key.as_ref())
      .await
      .ok()?
  }

  async fn remove_enforcer_result(&mut self, key: &PolicyCacheKey) {
    if let Err(err) = self.redis_client.del::<&str, ()>(key.as_ref()).await {
      error!("Failed to remove enforcer result from redis: {}", err);
    }
  }
}
