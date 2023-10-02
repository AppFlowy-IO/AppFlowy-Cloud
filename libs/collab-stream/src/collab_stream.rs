use crate::collab_update::{CollabUpdate, CollabUpdateRead, CollabUpdatesReadById, CreatedTime};
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, RedisError};
use std::borrow::Cow;

pub struct CollabStream {
  connection_manager: ConnectionManager,
  redis_stream_key: String,
}

impl CollabStream {
  pub fn new(connection_manager: ConnectionManager, oid: &str, partition_key: i64) -> Self {
    let redis_stream_key = stream_id_from_af_collab(oid, partition_key);
    Self {
      connection_manager,
      redis_stream_key,
    }
  }

  pub async fn read_all_updates(&mut self) -> Result<Vec<CollabUpdateRead>, redis::RedisError> {
    self
      .connection_manager
      .xrange_all(&self.redis_stream_key)
      .await
  }

  pub async fn insert_one_update(
    &mut self,
    collab_update: CollabUpdate,
  ) -> Result<CreatedTime, redis::RedisError> {
    self
      .connection_manager
      .xadd(
        &self.redis_stream_key,
        "*",
        &collab_update.to_single_tuple_array(),
      )
      .await
  }

  // returns the first instance of update after CreatedTime
  // if there is none, it blocks until there is one
  // if after is not specified, it returns the newest update
  pub async fn wait_one_update<'after>(
    &mut self,
    after: Option<CreatedTime>,
  ) -> Result<Vec<CollabUpdateRead>, redis::RedisError> {
    static NEWEST_ID: &str = "$";
    let keys = &self.redis_stream_key;
    let id: Cow<'after, str> = match after {
      Some(created_time) => Cow::Owned(format!(
        "{}-{}",
        created_time.timestamp_ms, created_time.sequence_number
      )),
      None => Cow::Borrowed(NEWEST_ID),
    };
    let options = redis::streams::StreamReadOptions::default().block(0);
    let mut update_by_id: CollabUpdatesReadById = self
      .connection_manager
      .xread_options(&[keys.as_str()], &[id.as_ref()], &options)
      .await?;
    let popped = update_by_id.0.pop_first().ok_or(RedisError::from((
      redis::ErrorKind::TypeError,
      "unexpected value from redis",
      format!("{:?}", update_by_id),
    )))?;
    Ok(popped.1)
  }
}

fn stream_id_from_af_collab(oid: &str, partition_key: i64) -> String {
  format!("af_collab-{}-{}", oid, partition_key)
}
