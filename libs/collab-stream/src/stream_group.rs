use crate::error::StreamError;
use crate::model::{CreatedTime, Message, MessageRead, MessageReadByStreamKey};
use redis::aio::ConnectionManager;
use redis::streams::{StreamMaxlen, StreamReadOptions};
use redis::{pipe, AsyncCommands, RedisError, RedisResult};

#[derive(Clone)]
pub struct CollabStreamGroup {
  connection_manager: ConnectionManager,
  stream_key: String,
  group_name: String,
}

impl CollabStreamGroup {
  pub fn new(
    workspace_id: &str,
    oid: &str,
    group_name: &str,
    connection_manager: ConnectionManager,
  ) -> Self {
    let group_name = group_name.to_string();
    let stream_key = format!("af_collab-{}-{}", workspace_id, oid);
    Self {
      group_name,
      connection_manager,
      stream_key,
    }
  }

  /// Ensures the consumer group exists, creating it if necessary.
  /// start_id:  
  ///   Use '$' if you want new messages or '0' to read from the beginning.
  pub async fn ensure_consumer_group(&mut self, start_id: &str) -> Result<(), StreamError> {
    let _: RedisResult<()> = self
      .connection_manager
      .xgroup_create_mkstream(&self.stream_key, &self.group_name, start_id)
      .await;

    Ok(())
  }

  /// Acknowledges messages processed by a consumer.
  pub async fn ack_messages(&mut self, message_ids: &[String]) -> Result<(), StreamError> {
    self
      .connection_manager
      .xack(&self.stream_key, &self.group_name, message_ids)
      .await?;
    Ok(())
  }

  /// Inserts a single message into the Redis stream.
  pub async fn insert_message(&mut self, message: Message) -> Result<CreatedTime, StreamError> {
    let tuple = message.into_tuple_array();
    let created_time = self
      .connection_manager
      .xadd(&self.stream_key, "*", tuple.as_slice())
      .await?;
    Ok(created_time)
  }

  /// Inserts multiple messages into the Redis stream using a pipeline.
  ///
  pub async fn insert_messages(&mut self, messages: Vec<Message>) -> Result<(), StreamError> {
    let mut pipe = pipe();
    for message in messages {
      let tuple = message.into_tuple_array();
      pipe.xadd(&self.stream_key, "*", tuple.as_slice());
    }
    pipe.query_async(&mut self.connection_manager).await?;
    Ok(())
  }

  /// Fetches number of messages from a Redis stream
  /// Returns the messages that were not consumed yet. Which means each message is delivered to only
  /// one consumer in the group
  pub async fn fetch_messages(
    &mut self,
    consumer_name: &str,
    count: usize,
  ) -> Result<Vec<Message>, StreamError> {
    let options = StreamReadOptions::default()
      .group(&self.group_name, consumer_name)
      .count(count)
      .block(100);

    let map: MessageReadByStreamKey = self
      .connection_manager
      .xread_options(&[&self.stream_key], &[">"], &options)
      .await?;

    match map.0.into_iter().next() {
      None => Ok(Vec::with_capacity(0)),
      Some((_, messages)) => Ok(messages.into_iter().map(Into::into).collect()),
    }
  }

  /// Reads all messages from the stream
  ///
  pub async fn read_all_message(&mut self) -> Result<Vec<Message>, StreamError> {
    let read_messages: Vec<MessageRead> =
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
