use crate::error::RealtimeError;
use futures_util::StreamExt;
use redis::{pipe, AsyncCommands, AsyncIter};

#[derive(Clone)]
pub struct RealtimeSharedState {
  redis_conn_manager: redis::aio::ConnectionManager,
  redis_key_prefix: String,
}

impl RealtimeSharedState {
  pub fn new(redis_conn_manager: redis::aio::ConnectionManager, redis_key_prefix: &str) -> Self {
    Self {
      redis_conn_manager,
      redis_key_prefix: redis_key_prefix.to_string(),
    }
  }
  pub async fn add_connected_user(&self, uid: i64, device_id: &str) -> Result<(), RealtimeError> {
    let mut conn = self.redis_conn_manager.clone();
    let key = self.realtime_shared_state_cache_key(&uid, device_id);
    conn
      .set_ex(key, "1", 60 * 60 * 3)
      .await
      .map_err(|err| RealtimeError::Internal(err.into()))?;
    Ok(())
  }

  pub async fn remove_connected_user(
    &self,
    uid: i64,
    device_id: &str,
  ) -> Result<(), RealtimeError> {
    let mut conn = self.redis_conn_manager.clone();
    let key = self.realtime_shared_state_cache_key(&uid, device_id);
    conn
      .del(key)
      .await
      .map_err(|err| RealtimeError::Internal(err.into()))?;
    Ok(())
  }

  pub async fn is_user_connected(&self, uid: &i64, device_id: &str) -> Result<bool, RealtimeError> {
    let mut conn = self.redis_conn_manager.clone();
    let key = self.realtime_shared_state_cache_key(uid, device_id);
    let result: Option<String> = conn
      .get(key)
      .await
      .map_err(|err| RealtimeError::Internal(err.into()))?;
    Ok(result.is_some())
  }

  pub async fn remove_all_connected_users(&self) -> Result<(), RealtimeError> {
    let mut conn = self.redis_conn_manager.clone();
    let iter: AsyncIter<String> = conn
      .scan_match(format!("{}:*", self.redis_key_prefix))
      .await
      .map_err(|err| RealtimeError::Internal(err.into()))?;
    let keys_to_delete: Vec<_> = iter.collect().await;
    if !keys_to_delete.is_empty() {
      let mut pipeline = pipe();
      for key in keys_to_delete.iter() {
        pipeline.del(key);
      }
      pipeline
        .query_async(&mut conn)
        .await
        .map_err(|err| RealtimeError::Internal(err.into()))?;
    }
    Ok(())
  }

  pub fn realtime_shared_state_cache_key(&self, uid: &i64, device_id: &str) -> String {
    format!("{}:{}:{}", self.redis_key_prefix, uid, device_id)
  }
}

pub const REALTIME_SHARE_STATE_V0_PREFIX: &str = "realtime_shared_state_v0";
pub const REALTIME_SHARE_STATE_V1_PREFIX: &str = "realtime_shared_state_v1";
