use crate::client::CollabRedisStream;
use crate::error::StreamError;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime};
use redis::aio::ConnectionManager;
use redis::{RedisResult, Value};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RELEASE_SCRIPT: &str = r#"
if redis.call("GET", KEYS[1]) == ARGV[1] then
  return redis.call("DEL", KEYS[1])
else
  return 0
end
"#;

pub struct LeaseAcquisition {
  conn: Option<ConnectionManager>,
  stream_key: String,
  token: u128,
}

impl LeaseAcquisition {
  pub async fn release(&mut self) -> Result<bool, StreamError> {
    if let Some(conn) = self.conn.take() {
      Self::release_internal(conn, &self.stream_key, self.token).await
    } else {
      Ok(false)
    }
  }

  async fn release_internal<S: AsRef<str>>(
    mut conn: ConnectionManager,
    stream_key: S,
    token: u128,
  ) -> Result<bool, StreamError> {
    let script = redis::Script::new(RELEASE_SCRIPT);
    let result: i32 = script
      .key(stream_key.as_ref())
      .arg(token.to_le_bytes().as_slice())
      .invoke_async(&mut conn)
      .await?;
    Ok(result == 1)
  }
}

impl Drop for LeaseAcquisition {
  fn drop(&mut self) {
    if let Some(conn) = self.conn.take() {
      tokio::spawn(Self::release_internal(
        conn,
        self.stream_key.clone(),
        self.token,
      ));
    }
  }
}

/// This is Redlock algorithm implementation.
/// See: https://redis.io/docs/latest/commands/set#patterns
#[async_trait]
pub trait Lease {
  /// Attempt to acquire lease on a stream for a given time-to-live.
  /// Returns `None` if the lease could not be acquired.
  async fn lease(
    &self,
    stream_key: String,
    ttl: Duration,
  ) -> Result<Option<LeaseAcquisition>, StreamError>;
}

#[async_trait]
impl Lease for ConnectionManager {
  async fn lease(
    &self,
    stream_key: String,
    ttl: Duration,
  ) -> Result<Option<LeaseAcquisition>, StreamError> {
    let mut conn = self.clone();
    let ttl = ttl.as_millis() as u64;
    let token = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_millis();
    tracing::trace!("acquiring lease `{}` for {}ms", stream_key, ttl);
    let result: Value = redis::cmd("SET")
      .arg(&stream_key)
      .arg(token.to_le_bytes().as_slice())
      .arg("NX")
      .arg("PX")
      .arg(ttl)
      .query_async(&mut conn)
      .await?;

    match result {
      Value::Okay => Ok(Some(LeaseAcquisition {
        conn: Some(conn),
        stream_key,
        token,
      })),
      o => {
        tracing::trace!("lease locked: {:?}", o);
        Ok(None)
      },
    }
  }
}

#[cfg(test)]
mod test {
  use crate::lease::Lease;
  use redis::Client;

  #[tokio::test]
  async fn lease_acquisition() {
    let redis_client = Client::open("redis://localhost:6379").unwrap();
    let mut conn = redis_client.get_connection_manager().await.unwrap();

    let l1 = conn
      .lease("stream1".into(), std::time::Duration::from_secs(1))
      .await
      .unwrap();

    assert!(l1.is_some(), "should successfully acquire lease");

    let l2 = conn
      .lease("stream1".into(), std::time::Duration::from_secs(1))
      .await
      .unwrap();

    assert!(l2.is_none(), "should fail to acquire lease");

    l1.unwrap().release().await.unwrap();

    let l3 = conn
      .lease("stream1".into(), std::time::Duration::from_secs(1))
      .await
      .unwrap();

    assert!(
      l3.is_some(),
      "should successfully acquire lease after it was released"
    );
  }
}
