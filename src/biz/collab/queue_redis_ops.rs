use crate::biz::collab::queue::PendingWriteMeta;
use crate::state::RedisConnectionManager;
use app_error::AppError;
use futures_util::StreamExt;
use redis::{AsyncCommands, AsyncIter, Script};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

pub(crate) const PENDING_WRITE_META_EXPIRE_SECS: u64 = 604800; // 7 days in seconds

pub(crate) async fn remove_all_pending_meta(
  mut connection_manager: RedisConnectionManager,
) -> Result<(), AppError> {
  let pattern = format!("{}*", QUEUE_COLLAB_PREFIX);
  let iter: AsyncIter<String> = connection_manager
    .scan_match(pattern)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;
  let keys: Vec<_> = iter.collect().await;

  if keys.is_empty() {
    return Ok(());
  }
  connection_manager
    .del(keys)
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

pub struct RedisSortedSet {
  conn: RedisConnectionManager,
  name: &'static str,
}

impl RedisSortedSet {
  pub fn new(conn: RedisConnectionManager, name: &'static str) -> Self {
    Self { conn, name }
  }

  pub async fn push(&self, item: PendingWrite) -> Result<(), anyhow::Error> {
    let data = serde_json::to_vec(&item)?;
    redis::cmd("ZADD")
      .arg(self.name)
      .arg(item.score())
      .arg(data)
      .query_async(&mut self.conn.clone())
      .await?;
    Ok(())
  }

  pub fn queue_name(&self) -> &'static str {
    self.name
  }

  pub async fn push_with_conn(
    &self,
    item: PendingWrite,
    conn: &mut RedisConnectionManager,
  ) -> Result<(), anyhow::Error> {
    let data = serde_json::to_vec(&item)?;
    redis::cmd("ZADD")
      .arg(self.name)
      .arg(item.score())
      .arg(data)
      .query_async(conn)
      .await?;
    Ok(())
  }

  /// Pop num of records from the queue
  /// The records are sorted by priority

  pub async fn pop(&self, len: usize) -> Result<Vec<PendingWrite>, anyhow::Error> {
    if len == 0 {
      return Ok(vec![]);
    }

    let script = Script::new(
      r#"
            local items = redis.call('ZRANGE', KEYS[1], 0, ARGV[1], 'WITHSCORES')
            if #items > 0 then
                redis.call('ZREMRANGEBYRANK', KEYS[1], 0, ARGV[1])
            end
            return items
            "#,
    );
    let mut conn = self.conn.clone();
    let items: Vec<(String, f64)> = script
      .key(self.name)
      .arg(len - 1)
      .invoke_async(&mut conn)
      .await?;

    let results = items
      .iter()
      .map(|(data, _score)| serde_json::from_str::<PendingWrite>(data).map_err(|e| e.into()))
      .collect::<Result<Vec<PendingWrite>, anyhow::Error>>()?;

    Ok(results)
  }

  pub async fn clear(&self) -> Result<(), anyhow::Error> {
    let mut conn = self.conn.clone();
    conn.del(self.name).await?;
    Ok(())
  }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PendingWrite {
  pub object_id: String,
  pub seq: i64,
  pub data_len: usize,
  pub priority: WritePriority,
}

impl PendingWrite {
  pub fn score(&self) -> i64 {
    match self.priority {
      WritePriority::High => 0,
      WritePriority::Low => self.seq + 1,
    }
  }
}

#[derive(Clone, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum WritePriority {
  High = 0,
  Low = 1,
}

impl Eq for PendingWrite {}
impl PartialEq for PendingWrite {
  fn eq(&self, other: &Self) -> bool {
    self.object_id == other.object_id
  }
}

impl Ord for PendingWrite {
  fn cmp(&self, other: &Self) -> std::cmp::Ordering {
    match (&self.priority, &other.priority) {
      (WritePriority::High, WritePriority::Low) => std::cmp::Ordering::Greater,
      (WritePriority::Low, WritePriority::High) => std::cmp::Ordering::Less,
      _ => {
        // Assuming lower seq is higher priority
        other.seq.cmp(&self.seq)
      },
    }
  }
}

impl PartialOrd for PendingWrite {
  fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
    Some(self.cmp(other))
  }
}

#[cfg(test)]
mod tests {
  use crate::biz::collab::{PendingWrite, RedisSortedSet, WritePriority};
  use anyhow::Context;
  use std::time::Duration;
  use tokio::time::sleep;

  #[tokio::test]
  async fn pending_write_sorted_set_test() {
    let conn = redis_client().await.get_connection_manager().await.unwrap();
    let sorted_set = RedisSortedSet::new(conn.clone(), "test_sorted_set");
    sorted_set.clear().await.unwrap();

    let pending_writes = vec![
      PendingWrite {
        object_id: "o1".to_string(),
        seq: 1,
        data_len: 0,
        priority: WritePriority::Low,
      },
      PendingWrite {
        object_id: "o2".to_string(),
        seq: 2,
        data_len: 0,
        priority: WritePriority::Low,
      },
      PendingWrite {
        object_id: "o3".to_string(),
        seq: 0,
        data_len: 0,
        priority: WritePriority::High,
      },
    ];

    for item in &pending_writes {
      sorted_set.push(item.clone()).await.unwrap();
    }

    let pending_writes_from_sorted_set = sorted_set.pop(3).await.unwrap();
    assert_eq!(pending_writes_from_sorted_set[0].object_id, "o3");
    assert_eq!(pending_writes_from_sorted_set[1].object_id, "o1");
    assert_eq!(pending_writes_from_sorted_set[2].object_id, "o2");

    let items = sorted_set.pop(2).await.unwrap();
    assert!(items.is_empty());
  }

  #[tokio::test]
  async fn sorted_set_consume_partial_items_test() {
    let conn = redis_client().await.get_connection_manager().await.unwrap();
    let sorted_set_1 = RedisSortedSet::new(conn.clone(), "test_sorted_set2");
    sorted_set_1.clear().await.unwrap();
    sleep(Duration::from_secs(3)).await;

    let pending_writes = vec![
      PendingWrite {
        object_id: "o1".to_string(),
        seq: 1,
        data_len: 0,
        priority: WritePriority::Low,
      },
      PendingWrite {
        object_id: "o1".to_string(),
        seq: 1,
        data_len: 0,
        priority: WritePriority::Low,
      },
      PendingWrite {
        object_id: "o2".to_string(),
        seq: 2,
        data_len: 0,
        priority: WritePriority::Low,
      },
      PendingWrite {
        object_id: "o3".to_string(),
        seq: 0,
        data_len: 0,
        priority: WritePriority::High,
      },
    ];

    for item in &pending_writes {
      sorted_set_1.push(item.clone()).await.unwrap();
    }

    let pending_writes_from_sorted_set = sorted_set_1.pop(1).await.unwrap();
    assert_eq!(pending_writes_from_sorted_set[0].object_id, "o3");

    let sorted_set_2 = RedisSortedSet::new(conn.clone(), "test_sorted_set2");
    let pending_writes_from_sorted_set = sorted_set_2.pop(10).await.unwrap();
    assert_eq!(pending_writes_from_sorted_set.len(), 2);
    assert_eq!(pending_writes_from_sorted_set[0].object_id, "o1");
    assert_eq!(pending_writes_from_sorted_set[1].object_id, "o2");

    assert!(sorted_set_1.pop(10).await.unwrap().is_empty());
    assert!(sorted_set_2.pop(10).await.unwrap().is_empty());
  }

  #[tokio::test]
  async fn larget_num_set_test() {
    let conn = redis_client().await.get_connection_manager().await.unwrap();
    let sorted_set = RedisSortedSet::new(conn.clone(), "test_sorted_set3");
    sorted_set.clear().await.unwrap();
    sleep(Duration::from_secs(3)).await;
    assert!(sorted_set.pop(10).await.unwrap().is_empty());

    for i in 0..100 {
      let pending_write = PendingWrite {
        object_id: format!("o{}", i),
        seq: i,
        data_len: 0,
        priority: WritePriority::Low,
      };
      sorted_set.push(pending_write).await.unwrap();

      if i == 20 {
        let set_1 = sorted_set.pop(20).await.unwrap();
        assert_eq!(set_1.len(), 20);
        assert_eq!(set_1[19].object_id, "o19");
      }
    }

    let set_2 = sorted_set.pop(30).await.unwrap();
    assert_eq!(set_2.len(), 30);
    assert_eq!(set_2[0].object_id, "o20");
    assert_eq!(set_2[29].object_id, "o49");

    let set_3 = sorted_set.pop(1).await.unwrap();
    assert_eq!(set_3.len(), 1);
    assert_eq!(set_3[0].object_id, "o50");

    let set_4 = sorted_set.pop(200).await.unwrap();
    assert_eq!(set_4.len(), 49);
  }

  async fn redis_client() -> redis::Client {
    let redis_uri = "redis://localhost:6379";
    redis::Client::open(redis_uri)
      .context("failed to connect to redis")
      .unwrap()
  }
}
