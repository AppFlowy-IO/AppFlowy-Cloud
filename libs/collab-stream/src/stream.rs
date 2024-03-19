use crate::error::StreamError;
use crate::model::{CreatedTime, Message, MessageRead, MessageReadById};
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, RedisError};
use std::borrow::Cow;

pub struct CollabStream {
  connection_manager: ConnectionManager,
  stream_key: String,
}

impl CollabStream {
  pub fn new(connection_manager: ConnectionManager, workspace_id: &str, oid: &str) -> Self {
    let stream_key = format!("af_collab-{}-{}", workspace_id, oid);
    Self {
      connection_manager,
      stream_key,
    }
  }

  pub async fn read_all_message(&mut self) -> Result<Vec<MessageRead>, RedisError> {
    self.connection_manager.xrange_all(&self.stream_key).await
  }

  pub async fn insert_message(&mut self, message: Message) -> Result<CreatedTime, RedisError> {
    self
      .connection_manager
      .xadd(&self.stream_key, "*", &message.to_single_tuple_array())
      .await
  }

  // returns the first instance of update after CreatedTime
  // if there is none, it blocks until there is one
  // if after is not specified, it returns the newest update
  pub async fn wait_one_update<'after>(
    &mut self,
    after: Option<CreatedTime>,
  ) -> Result<Vec<MessageRead>, StreamError> {
    static NEWEST_ID: &str = "$";
    let keys = &self.stream_key;
    let id: Cow<'after, str> = match after {
      Some(created_time) => Cow::Owned(format!(
        "{}-{}",
        created_time.timestamp_ms, created_time.sequence_number
      )),
      None => Cow::Borrowed(NEWEST_ID),
    };
    let options = redis::streams::StreamReadOptions::default().block(0);
    let mut message_by_id: MessageReadById = self
      .connection_manager
      .xread_options(&[keys.as_str()], &[id.as_ref()], &options)
      .await?;
    let popped = message_by_id
      .0
      .pop_first()
      .ok_or(StreamError::UnexpectedValue(format!("{:?}", message_by_id)))?;
    Ok(popped.1)
  }
}
