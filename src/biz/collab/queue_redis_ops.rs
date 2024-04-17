use crate::biz::collab::queue::{PendingWrite, PendingWriteMeta};
use crate::state::RedisConnectionManager;
use app_error::AppError;
use collab_entity::CollabType;
use futures_util::StreamExt;
use redis::{AsyncCommands, AsyncIter};
use std::str::FromStr;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

pub(crate) const PENDING_WRITE_EXPIRE_IN_SECS: u64 = 604800; // 7 days in seconds
#[inline]
pub(crate) async fn insert_pending_meta(
  uid: &i64,
  workspace_id: &str,
  object_id: &str,
  data_len: usize,
  collab_type: &CollabType,
  mut connection_manager: RedisConnectionManager,
) -> Result<(), AppError> {
  let queue_item = PendingWriteMeta {
    uid: *uid,
    workspace_id: workspace_id.to_string(),
    object_id: object_id.to_string(),
    collab_type: collab_type.clone(),
  };

  let key = storage_cache_key(&queue_item.object_id, data_len);
  let value = serde_json::to_string(&queue_item)?;

  // set the key with a timeout of 7 days
  connection_manager
    .set_ex(&key, &value, PENDING_WRITE_EXPIRE_IN_SECS)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;

  Ok(())
}

#[inline]
pub(crate) async fn get_pending_meta(
  keys: &[String],
  connection_manager: &mut RedisConnectionManager,
) -> Result<Vec<PendingWriteMeta>, AppError> {
  let results: Vec<Option<String>> = connection_manager
    .get(keys)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;

  let metas = results
    .into_iter()
    .filter_map(|value| value.and_then(|data| serde_json::from_str(&data).ok()))
    .collect::<Vec<PendingWriteMeta>>();

  Ok(metas)
}

pub(crate) async fn get_remaining_pending_write(
  counter: &Arc<AtomicI64>,
  connection_manager: &mut RedisConnectionManager,
) -> Result<Vec<PendingWrite>, AppError> {
  let keys = scan_all_keys(connection_manager).await?;
  Ok(
    keys
      .into_iter()
      .flat_map(|cache_object_id| {
        pending_write_from_cache_object_id(&cache_object_id, counter).ok()
      })
      .collect::<Vec<_>>(),
  )
}

async fn scan_all_keys(conn_manager: &mut RedisConnectionManager) -> Result<Vec<String>, AppError> {
  let pattern = format!("{}*", QUEUE_COLLAB_PREFIX);
  let iter: AsyncIter<String> = conn_manager
    .scan_match(pattern)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;
  let keys: Vec<_> = iter.collect().await;
  Ok(keys)
}

#[inline]
pub(crate) async fn remove_pending_meta(
  keys: &[String],
  connection_manager: &mut RedisConnectionManager,
) -> Result<(), AppError> {
  connection_manager
    .del(keys)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;
  Ok(())
}

pub(crate) const QUEUE_COLLAB_PREFIX: &str = "storage_pending_meta_v0:";

#[inline]
pub(crate) fn storage_cache_key(object_id: &str, data_len: usize) -> String {
  format!("{}{}:{}", QUEUE_COLLAB_PREFIX, object_id, data_len)
}

pub(crate) fn pending_write_from_cache_object_id(
  key: &str,
  counter: &Arc<AtomicI64>,
) -> Result<PendingWrite, String> {
  let trimmed_key = key.trim_start_matches(QUEUE_COLLAB_PREFIX);
  let parts: Vec<&str> = trimmed_key.split(':').collect();

  if parts.len() != 2 {
    return Err(format!("Invalid key format: {}", key));
  }

  let object_id = parts[0].to_owned();
  let data_len = usize::from_str(parts[1]).map_err(|e| e.to_string())?;

  Ok(PendingWrite {
    object_id,
    seq: counter.fetch_add(1, Ordering::SeqCst),
    data_len,
  })
}

#[cfg(test)]
mod tests {
  use crate::biz::collab::queue::PendingWriteMeta;
  use crate::biz::collab::queue_redis_ops::{
    get_pending_meta, get_remaining_pending_write, insert_pending_meta, remove_pending_meta,
    storage_cache_key,
  };
  use anyhow::Context;
  use collab_entity::CollabType;
  use std::sync::atomic::AtomicI64;
  use std::sync::Arc;

  #[tokio::test]
  async fn pending_write_meta_insert_get_test() {
    let data_len = 100;
    let mut conn = redis_client().await.get_connection_manager().await.unwrap();
    let metas = vec![
      PendingWriteMeta {
        uid: 1,
        workspace_id: "w1".to_string(),
        object_id: "o1".to_string(),
        collab_type: CollabType::Document,
      },
      PendingWriteMeta {
        uid: 2,
        workspace_id: "w2".to_string(),
        object_id: "o2".to_string(),
        collab_type: CollabType::Document,
      },
    ];

    for meta in metas.iter() {
      insert_pending_meta(
        &meta.uid,
        &meta.workspace_id,
        &meta.object_id,
        data_len,
        &meta.collab_type,
        conn.clone(),
      )
      .await
      .unwrap();
    }

    let keys = metas
      .iter()
      .map(|meta| storage_cache_key(&meta.object_id, data_len))
      .collect::<Vec<String>>();

    let metas_from_cache = get_pending_meta(&keys, &mut conn).await.unwrap();
    assert_eq!(metas, metas_from_cache);

    remove_pending_meta(&keys, &mut conn).await.unwrap();
    let metas = get_pending_meta(&keys, &mut conn).await.unwrap();
    assert!(metas.is_empty());
  }

  #[tokio::test]
  async fn get_all_remaining_write_test() {
    let mut conn = redis_client().await.get_connection_manager().await.unwrap();
    let o1 = uuid::Uuid::new_v4().to_string();
    let o2 = uuid::Uuid::new_v4().to_string();
    insert_pending_meta(&1, "w1", &o1, 100, &CollabType::Document, conn.clone())
      .await
      .unwrap();
    insert_pending_meta(&2, "w2", &o2, 101, &CollabType::Document, conn.clone())
      .await
      .unwrap();

    let counter = Arc::new(AtomicI64::new(0));
    let pending_writes = get_remaining_pending_write(&counter, &mut conn)
      .await
      .unwrap()
      .into_iter()
      .filter(|item| item.object_id == o1 || item.object_id == o2)
      .collect::<Vec<_>>();

    // the returns pending writes are not guaranteed to be in order
    assert_eq!(pending_writes.len(), 2);
    // assert the content of the pending writes
    assert!(pending_writes.iter().any(|item| item.object_id == o1));
    assert!(pending_writes.iter().any(|item| item.object_id == o2));

    let keys = pending_writes
      .iter()
      .map(|item| storage_cache_key(&item.object_id, item.data_len))
      .collect::<Vec<String>>();
    remove_pending_meta(&keys, &mut conn).await.unwrap();

    let pending_writes = get_remaining_pending_write(&counter, &mut conn)
      .await
      .unwrap()
      .into_iter()
      .filter(|item| item.object_id == o1 || item.object_id == o2)
      .collect::<Vec<_>>();
    assert!(
      pending_writes.is_empty(),
      "expect no pending writes, but got {:?}",
      pending_writes.len()
    );
  }

  async fn redis_client() -> redis::Client {
    let redis_uri = "redis://localhost:6379";
    redis::Client::open(redis_uri)
      .context("failed to connect to redis")
      .unwrap()
  }
}
