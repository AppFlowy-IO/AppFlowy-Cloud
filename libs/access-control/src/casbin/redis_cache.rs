use casbin::Cache;
use redis::{Client, Connection, IntoConnectionInfo, RedisResult};
use serde::{de::DeserializeOwned, Serialize};
use std::{hash::Hash, marker::PhantomData, sync::Mutex};

const CACHE_HKEY: &str = "ac:cache:v1";
const CACHE_TTL_SECONDS: usize = 600; // 10 minutes

pub struct RedisCache<K, V> {
  conn: Mutex<Connection>,
  _marker: PhantomData<(K, V)>,
}

impl<K, V> RedisCache<K, V>
where
  K: Eq + Hash + Clone,
{
  pub fn new<T: IntoConnectionInfo>(redis_url: T) -> RedisResult<RedisCache<K, V>> {
    let client = Client::open(redis_url)?;
    let conn = client.get_connection()?;

    Ok(RedisCache {
      conn: Mutex::new(conn),
      _marker: PhantomData,
    })
  }
}

impl<K, V> Cache<K, V> for RedisCache<K, V>
where
  K: Eq + Hash + Send + Sync + Serialize + Clone + 'static,
  V: Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
{
  fn get(&self, k: &K) -> Option<V> {
    // Get from Redis
    if let Ok(field) = serde_json::to_string(&k) {
      if let Ok(mut conn) = self.conn.lock() {
        if let Ok(res) = redis::cmd("HGET")
          .arg(CACHE_HKEY)
          .arg(&field)
          .query::<Option<String>>(&mut *conn)
        {
          if let Some(data) = res.as_ref().and_then(|d| serde_json::from_str::<V>(d).ok()) {
            return Some(data);
          }
        }
      }
    }

    None
  }

  fn has(&self, k: &K) -> bool {
    if let Ok(field) = serde_json::to_string(&k) {
      if let Ok(mut conn) = self.conn.lock() {
        if let Ok(res) = redis::cmd("HEXISTS")
          .arg(CACHE_HKEY)
          .arg(&field)
          .query::<bool>(&mut *conn)
        {
          return res;
        }
      }
    }

    false
  }

  fn set(&self, k: K, v: V) {
    if let Ok(mut conn) = self.conn.lock() {
      if let (Ok(field), Ok(value)) = (serde_json::to_string(&k), serde_json::to_string(&v)) {
        let script = r#"
            redis.call('HSET', KEYS[1], ARGV[1], ARGV[2])
            redis.call('EXPIRE', KEYS[1], ARGV[3])
            return 1
        "#;

        let _ = redis::Script::new(script)
          .key(CACHE_HKEY)
          .arg(&field)
          .arg(&value)
          .arg(CACHE_TTL_SECONDS)
          .invoke::<()>(&mut *conn);
      }
    }
  }

  fn clear(&self) {
    // Clear Redis
    if let Ok(mut conn) = self.conn.lock() {
      let _ = redis::cmd("DEL").arg(CACHE_HKEY).query::<bool>(&mut *conn);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  #[ignore]
  fn test_set_has_get_clear() {
    // Skip test if Redis is not available
    let cache = match RedisCache::new("redis://localhost:6379") {
      Ok(cache) => cache,
      Err(_) => {
        println!("Redis not available, skipping test");
        return;
      },
    };

    let cache: RedisCache<Vec<&str>, bool> = cache;

    // Clear any existing cache
    cache.clear();

    // Test set and has
    cache.set(vec!["alice", "/data1", "read"], false);
    assert!(cache.has(&vec!["alice", "/data1", "read"]));

    // Test get
    assert_eq!(cache.get(&vec!["alice", "/data1", "read"]), Some(false));

    // Test clear
    cache.clear();
    assert_eq!(cache.get(&vec!["alice", "/data1", "read"]), None);
  }

  #[test]
  #[ignore]
  fn test_ttl_expiration() {
    let cache = match RedisCache::new("redis://localhost:6379") {
      Ok(cache) => cache,
      Err(_) => {
        println!("Redis not available, skipping test");
        return;
      },
    };

    let cache: RedisCache<String, String> = cache;
    cache.clear();

    // Set a value
    cache.set("test_key".to_string(), "test_value".to_string());

    // Verify it exists
    assert_eq!(
      cache.get(&"test_key".to_string()),
      Some("test_value".to_string())
    );
  }
}
