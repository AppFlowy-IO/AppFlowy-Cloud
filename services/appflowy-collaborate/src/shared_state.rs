use crate::error::RealtimeError;
use futures_util::StreamExt;
use redis::{pipe, AsyncCommands, AsyncIter};

#[derive(Clone)]
pub struct RealtimeSharedState {
  redis_conn_manager: redis::aio::ConnectionManager,
}

impl RealtimeSharedState {
  pub fn new(redis_conn_manager: redis::aio::ConnectionManager) -> Self {
    Self { redis_conn_manager }
  }
  pub async fn add_connected_user(&self, uid: i64, device_id: &str) -> Result<(), RealtimeError> {
    let mut conn = self.redis_conn_manager.clone();
    let key = realtime_shared_state_cache_key(&uid, device_id);
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
    let key = realtime_shared_state_cache_key(&uid, device_id);
    conn
      .del(key)
      .await
      .map_err(|err| RealtimeError::Internal(err.into()))?;
    Ok(())
  }

  pub async fn is_user_connected(&self, uid: &i64, device_id: &str) -> Result<bool, RealtimeError> {
    let mut conn = self.redis_conn_manager.clone();
    let key = realtime_shared_state_cache_key(uid, device_id);
    let result: Option<String> = conn
      .get(key)
      .await
      .map_err(|err| RealtimeError::Internal(err.into()))?;
    Ok(result.is_some())
  }

  pub async fn remove_all_connected_users(&self) -> Result<(), RealtimeError> {
    let mut conn = self.redis_conn_manager.clone();
    let iter: AsyncIter<String> = conn
      .scan_match(format!("{}:*", REALTIME_SHARE_STATE_PREFIX))
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
}

pub(crate) const REALTIME_SHARE_STATE_PREFIX: &str = "realtime_shared_state_v0";

#[inline]
pub(crate) fn realtime_shared_state_cache_key(uid: &i64, device_id: &str) -> String {
  format!("{}:{}:{}", REALTIME_SHARE_STATE_PREFIX, uid, device_id)
}
