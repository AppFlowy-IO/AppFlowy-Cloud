use crate::state::RedisConnectionManager;
use anyhow::anyhow;
use app_error::AppError;
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub(crate) struct SnapshotCache {
  redis_client: Arc<Mutex<RedisConnectionManager>>,
}

impl SnapshotCache {
  pub fn new(redis_client: Arc<Mutex<RedisConnectionManager>>) -> Self {
    Self { redis_client }
  }

  /// Returns all existing keys start with `prefix`
  #[allow(dead_code)]
  pub async fn keys(&self, prefix: &str) -> Result<Vec<String>, AppError> {
    let mut redis = self.redis_client.lock().await;
    let keys: Vec<String> = redis
      .keys(format!("{}*", prefix))
      .await
      .map_err(|err| AppError::Internal(err.into()))?;
    Ok(keys)
  }

  pub async fn insert(&self, key: &str, value: Vec<u8>) -> Result<(), AppError> {
    let mut redis = self.redis_client.lock().await;
    redis
      .set_ex(key, value, 60 * 60 * 24)
      .await
      .map_err(|err| AppError::Internal(err.into()))?;
    Ok(())
  }

  pub async fn try_get(&self, key: &str) -> Result<Option<Vec<u8>>, AppError> {
    let mut redis = self
      .redis_client
      .try_lock()
      .map_err(|_| AppError::Internal(anyhow!("lock error")))?;
    let value = redis
      .get::<_, Option<Vec<u8>>>(key)
      .await
      .map_err(|err| AppError::Internal(err.into()))?;
    Ok(value)
  }

  pub async fn remove(&self, key: &str) -> Result<(), AppError> {
    let mut redis = self.redis_client.lock().await;
    redis
      .del(key)
      .await
      .map_err(|err| AppError::Internal(err.into()))?;
    Ok(())
  }
}
