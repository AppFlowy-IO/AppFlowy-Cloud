use crate::error::StreamError;
use crate::model::{MessageId, StreamBinary, StreamMessage, StreamMessageByStreamKey};
use redis::aio::ConnectionManager;
use redis::streams::{StreamMaxlen, StreamReadOptions};
use redis::{pipe, AsyncCommands, RedisError};

pub struct CollabStream {
  connection_manager: ConnectionManager,
  stream_key: String,
}

impl CollabStream {
  pub fn new(workspace_id: &str, oid: &str, connection_manager: ConnectionManager) -> Self {
    let stream_key = format!("af_collab-{}-{}", workspace_id, oid);
    Self {
      connection_manager,
      stream_key,
    }
  }

  /// Inserts a single message into the Redis stream.
  pub async fn insert_message(&mut self, message: StreamBinary) -> Result<MessageId, StreamError> {
    let tuple = message.into_tuple_array();
    let message_id = self
      .connection_manager
      .xadd(&self.stream_key, "*", tuple.as_slice())
      .await?;
    Ok(message_id)
  }

  /// Inserts multiple messages into the Redis stream using a pipeline.
  ///
  pub async fn insert_messages(&mut self, messages: Vec<StreamBinary>) -> Result<(), StreamError> {
    let mut pipe = pipe();
    for message in messages {
      let tuple = message.into_tuple_array();
      pipe.xadd(&self.stream_key, "*", tuple.as_slice());
    }
    pipe.query_async(&mut self.connection_manager).await?;
    Ok(())
  }

  /// Fetches the next message from a Redis stream after a specified entry.
  ///
  pub async fn next(&mut self) -> Result<Option<StreamMessage>, StreamError> {
    let options = StreamReadOptions::default().count(1).block(100);
    let map: StreamMessageByStreamKey = self
      .connection_manager
      .xread_options(&[&self.stream_key], &["$"], &options)
      .await?;

    let (_, mut messages) = map
      .0
      .into_iter()
      .next()
      .ok_or_else(|| StreamError::UnexpectedValue("Empty stream".into()))?;

    debug_assert_eq!(messages.len(), 1);
    Ok(messages.pop())
  }

  pub async fn next_after(
    &mut self,
    after: Option<MessageId>,
  ) -> Result<Option<StreamMessage>, StreamError> {
    let message_id = after
      .map(|ct| ct.to_string())
      .unwrap_or_else(|| "$".to_string());

    let options = StreamReadOptions::default().group("1", "2").block(100);
    let map: StreamMessageByStreamKey = self
      .connection_manager
      .xread_options(&[&self.stream_key], &[&message_id], &options)
      .await?;

    let (_, mut messages) = map
      .0
      .into_iter()
      .next()
      .ok_or_else(|| StreamError::UnexpectedValue("Empty stream".into()))?;

    debug_assert_eq!(messages.len(), 1);
    Ok(messages.pop())
  }

  pub async fn read_all_message(&mut self) -> Result<Vec<StreamBinary>, StreamError> {
    let read_messages: Vec<StreamMessage> =
      self.connection_manager.xrange_all(&self.stream_key).await?;
    Ok(read_messages.into_iter().map(Into::into).collect())
  }

  pub async fn clear(&mut self) -> Result<(), RedisError> {
    self
      .connection_manager
      .xtrim(&self.stream_key, StreamMaxlen::Equals(0))
      .await?;
    Ok(())
  }
}
