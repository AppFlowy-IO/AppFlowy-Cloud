use crate::collab::queue::PendingWriteMeta;
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
  let results: Vec<Option<Vec<u8>>> = connection_manager
    .get(keys)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;

  let metas = results
    .into_iter()
    .filter_map(|value| value.and_then(|data| serde_json::from_slice(&data).ok()))
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

#[derive(Clone)]
pub struct RedisSortedSet {
  conn: RedisConnectionManager,
  name: String,
}

impl RedisSortedSet {
  pub fn new(conn: RedisConnectionManager, name: &str) -> Self {
    Self {
      conn,
      name: name.to_string(),
    }
  }

  pub async fn push(&self, item: PendingWrite) -> Result<(), anyhow::Error> {
    let data = serde_json::to_vec(&item)?;
    redis::cmd("ZADD")
      .arg(&self.name)
      .arg(item.score())
      .arg(data)
      .query_async(&mut self.conn.clone())
      .await?;
    Ok(())
  }

  pub async fn push_with_conn(
    &self,
    item: PendingWrite,
    conn: &mut RedisConnectionManager,
  ) -> Result<(), anyhow::Error> {
    let data = serde_json::to_vec(&item)?;
    redis::cmd("ZADD")
      .arg(&self.name)
      .arg(item.score())
      .arg(data)
      .query_async(conn)
      .await?;
    Ok(())
  }

  pub fn queue_name(&self) -> &str {
    &self.name
  }

  /// Pops items from a Redis sorted set.
  ///
  /// This asynchronous function retrieves and removes the top `len` items from a Redis sorted set specified by `self.name`.
  /// It uses a Lua script to atomically perform the operation to maintain data integrity during concurrent access.
  ///
  /// # Parameters
  /// - `len`: The number of items to pop from the sorted set. If `len` is 0, the function returns an empty vector.
  ///
  pub async fn pop(&self, len: usize) -> Result<Vec<PendingWrite>, anyhow::Error> {
    if len == 0 {
      return Ok(vec![]);
    }

    let script = Script::new(
      r#"
         local items = redis.call('ZRANGE', KEYS[1], 0, ARGV[1], 'WITHSCORES')
         if #items > 0 then
           redis.call('ZREMRANGEBYRANK', KEYS[1], 0, #items / 2 - 1)
         end
         return items
       "#,
    );
    let mut conn = self.conn.clone();
    let items: Vec<(String, f64)> = script
      .key(&self.name)
      .arg(len - 1)
      .invoke_async(&mut conn)
      .await?;

    let results = items
      .iter()
      .map(|(data, _score)| serde_json::from_str::<PendingWrite>(data).map_err(|e| e.into()))
      .collect::<Result<Vec<PendingWrite>, anyhow::Error>>()?;

    Ok(results)
  }

  pub async fn peek(&self, n: usize) -> Result<Vec<PendingWrite>, anyhow::Error> {
    let mut conn = self.conn.clone();
    let items: Vec<(String, f64)> = redis::cmd("ZREVRANGE")
      .arg(&self.name)
      .arg(0)
      .arg(n - 1)
      .arg("WITHSCORES")
      .query_async(&mut conn)
      .await?;

    let results = items
      .iter()
      .map(|(data, _score)| serde_json::from_str::<PendingWrite>(data).map_err(|e| e.into()))
      .collect::<Result<Vec<PendingWrite>, anyhow::Error>>()?;

    Ok(results)
  }
  pub async fn remove_items<T: AsRef<str>>(
    &self,
    items_to_remove: Vec<T>,
  ) -> Result<(), anyhow::Error> {
    let mut conn = self.conn.clone();
    let mut pipe = redis::pipe();
    for item in items_to_remove {
      pipe.cmd("ZREM").arg(&self.name).arg(item.as_ref()).ignore();
    }
    pipe.query_async::<_, ()>(&mut conn).await?;
    Ok(())
  }

  pub async fn clear(&self) -> Result<(), anyhow::Error> {
    let mut conn = self.conn.clone();
    conn.del(&self.name).await?;
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
  use crate::collab::{PendingWrite, RedisSortedSet, WritePriority};
  use anyhow::Context;
  use std::time::Duration;

  #[tokio::test]
  async fn pending_write_sorted_set_test() {
    let conn = redis_client().await.get_connection_manager().await.unwrap();
    let set_name = uuid::Uuid::new_v4().to_string();
    let sorted_set = RedisSortedSet::new(conn.clone(), &set_name);

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
    let set_name = uuid::Uuid::new_v4().to_string();
    let sorted_set_1 = RedisSortedSet::new(conn.clone(), &set_name);

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

    let sorted_set_2 = RedisSortedSet::new(conn.clone(), &set_name);
    let pending_writes_from_sorted_set = sorted_set_2.pop(10).await.unwrap();
    assert_eq!(pending_writes_from_sorted_set.len(), 2);
    assert_eq!(pending_writes_from_sorted_set[0].object_id, "o1");
    assert_eq!(pending_writes_from_sorted_set[1].object_id, "o2");

    assert!(sorted_set_1.pop(10).await.unwrap().is_empty());
    assert!(sorted_set_2.pop(10).await.unwrap().is_empty());
  }

  #[tokio::test]
  async fn large_num_set_test() {
    let conn = redis_client().await.get_connection_manager().await.unwrap();
    let set_name = uuid::Uuid::new_v4().to_string();
    let sorted_set = RedisSortedSet::new(conn.clone(), &set_name);
    assert!(sorted_set.pop(10).await.unwrap().is_empty());

    for i in 0..100 {
      let pending_write = PendingWrite {
        object_id: format!("o{}", i),
        seq: i,
        data_len: 0,
        priority: WritePriority::Low,
      };
      sorted_set.push(pending_write).await.unwrap();
    }

    let set_1 = sorted_set.pop(20).await.unwrap();
    assert_eq!(set_1.len(), 20);
    assert_eq!(set_1[19].object_id, "o19");

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

  #[tokio::test]
  async fn multi_threads_sorted_set_test() {
    let conn = redis_client().await.get_connection_manager().await.unwrap();
    let set_name = uuid::Uuid::new_v4().to_string();
    let sorted_set = RedisSortedSet::new(conn.clone(), &set_name);

    let mut handles = vec![];
    for i in 0..100 {
      let cloned_sorted_set = sorted_set.clone();
      let handle = tokio::spawn(async move {
        let pending_write = PendingWrite {
          object_id: format!("o{}", i),
          seq: i,
          data_len: 0,
          priority: WritePriority::Low,
        };
        tokio::time::sleep(Duration::from_millis(rand::random::<u64>() % 100)).await;
        cloned_sorted_set
          .push(pending_write)
          .await
          .expect("Failed to push data")
      });
      handles.push(handle);
    }
    futures::future::join_all(handles).await;

    let mut handles = vec![];
    for _ in 0..10 {
      let cloned_sorted_set = sorted_set.clone();
      let handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(rand::random::<u64>() % 100)).await;
        let items = cloned_sorted_set
          .pop(10)
          .await
          .expect("Failed to pop items");
        assert_eq!(items.len(), 10, "Expected exactly 10 items to be popped");
      });
      handles.push(handle);
    }
    let results = futures::future::join_all(handles).await;
    for result in results {
      result.expect("A thread panicked or errored out");
    }
  }

  async fn redis_client() -> redis::Client {
    let redis_uri = "redis://localhost:6379";
    redis::Client::open(redis_uri)
      .context("failed to connect to redis")
      .unwrap()
  }
}
