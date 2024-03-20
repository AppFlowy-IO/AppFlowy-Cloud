use crate::error::StreamError;
use crate::model::{Message, MessageId, StreamMessage, StreamMessageByStreamKey};
use redis::aio::ConnectionManager;
use redis::streams::{StreamMaxlen, StreamPendingData, StreamPendingReply, StreamReadOptions};
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
  ///
  /// In Redis streams, when a message is delivered to a consumer using XREADGROUP, it moves into
  /// a pending state for that consumer. Redis expects you to manually acknowledge these messages
  /// using XACK once they have been successfully processed. If you don't acknowledge a message,
  /// it remains in the pending state for that consumer. Redis keeps track of these messages so you
  /// can handle message failures or retries.
  pub async fn ack_messages(&mut self, message_ids: &[String]) -> Result<(), StreamError> {
    self
      .connection_manager
      .xack(&self.stream_key, &self.group_name, message_ids)
      .await?;
    Ok(())
  }

  /// Inserts a single message into the Redis stream.
  pub async fn insert_message(&mut self, message: Message) -> Result<MessageId, StreamError> {
    let tuple = message.into_tuple_array();
    let message_id = self
      .connection_manager
      .xadd(&self.stream_key, "*", tuple.as_slice())
      .await?;
    Ok(message_id)
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
  ///
  /// $: This symbol is used with the XREAD command to indicate that you want to start reading only
  /// new messages that arrive in the stream after the read command has been issued. Essentially,
  /// it tells Redis to ignore all the messages already in the stream and only listen for new ones.
  /// It's particularly useful when you want to start processing messages from the current moment
  /// forward and don't need to process historical messages.
  ///
  /// >: This symbol is used with the XREADGROUP command in the context of consumer groups. When a
  /// consumer group reads from a stream using >, it tells Redis to deliver only messages that have
  /// not yet been acknowledged by any consumer in the group. This allows different consumers in the
  /// group to read and process different messages concurrently, without receiving messages that have
  /// already been processed by another consumer. It's a way to distribute the workload of processing
  /// stream messages across multiple consumers.
  pub async fn consumer_messages(
    &mut self,
    consumer_name: &str,
    option: ConsumeOptions,
  ) -> Result<Vec<StreamMessage>, StreamError> {
    let mut options = StreamReadOptions::default()
      .group(&self.group_name, consumer_name)
      .block(100);

    let mut message_id = ">".to_string();
    match option {
      ConsumeOptions::Empty => {},
      ConsumeOptions::Count(count) => {
        options = options.count(count);
      },
      ConsumeOptions::After(after) => {
        message_id = after.to_string();
      },
    }

    let map: StreamMessageByStreamKey = self
      .connection_manager
      .xread_options(&[&self.stream_key], &[message_id], &options)
      .await?;

    match map.0.into_iter().next() {
      None => Ok(Vec::with_capacity(0)),
      Some((_, messages)) => Ok(messages),
    }
  }

  /// Get messages starting from a specific message id.
  /// returns list of messages excluding the message with the start_id
  pub async fn get_messages_starting_from_id(
    &mut self,
    start_id: Option<String>,
    count: usize,
  ) -> Result<Vec<StreamMessage>, StreamError> {
    let options = StreamReadOptions::default().count(count).block(100);
    let message_id = start_id.unwrap_or_else(|| "0".to_string());
    let map: StreamMessageByStreamKey = self
      .connection_manager
      .xread_options(&[&self.stream_key], &[message_id], &options)
      .await?;

    match map.0.into_iter().next() {
      None => Ok(Vec::with_capacity(0)),
      Some((_, messages)) => Ok(messages),
    }
  }

  /// Reads all messages from the stream
  ///
  pub async fn get_all_message(&mut self) -> Result<Vec<StreamMessage>, StreamError> {
    let read_messages: Vec<StreamMessage> =
      self.connection_manager.xrange_all(&self.stream_key).await?;
    Ok(read_messages.into_iter().collect())
  }

  pub async fn pending_reply(&mut self) -> Result<Option<StreamPendingData>, StreamError> {
    let reply: StreamPendingReply = self
      .connection_manager
      .xpending(&self.stream_key, &self.group_name)
      .await?;

    match reply {
      StreamPendingReply::Empty => Ok(None),
      StreamPendingReply::Data(data) => Ok(Some(data)),
    }
  }

  pub async fn clear(&mut self) -> Result<(), RedisError> {
    self
      .connection_manager
      .xtrim(&self.stream_key, StreamMaxlen::Equals(0))
      .await?;
    Ok(())
  }
}

pub enum ConsumeOptions {
  Empty,
  Count(usize),
  After(MessageId),
}
