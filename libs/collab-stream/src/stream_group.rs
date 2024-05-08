use crate::error::StreamError;
use crate::model::{MessageId, StreamBinary, StreamMessage, StreamMessageByStreamKey};
use redis::aio::ConnectionManager;
use redis::streams::{
  StreamClaimOptions, StreamClaimReply, StreamMaxlen, StreamPendingData, StreamPendingReply,
  StreamReadOptions,
};
use redis::{pipe, AsyncCommands, RedisResult};
use tracing::{error, trace};

#[derive(Clone)]
pub struct StreamGroup {
  connection_manager: ConnectionManager,
  stream_key: String,
  group_name: String,
}

impl StreamGroup {
  pub fn new(stream_key: String, group_name: &str, connection_manager: ConnectionManager) -> Self {
    let group_name = group_name.to_string();
    Self {
      group_name,
      connection_manager,
      stream_key,
    }
  }

  /// Ensures the consumer group exists, creating it if necessary.
  pub async fn ensure_consumer_group(&mut self) -> Result<(), StreamError> {
    let _: RedisResult<()> = self
      .connection_manager
       //Use '$' if you want new messages or '0' to read from the beginning.
      .xgroup_create_mkstream(&self.stream_key, &self.group_name, "0")
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
  pub async fn ack_message_ids<T: ToString>(
    &mut self,
    message_ids: Vec<T>,
  ) -> Result<(), StreamError> {
    if message_ids.is_empty() {
      return Ok(());
    }
    let message_ids = message_ids
      .into_iter()
      .map(|m| m.to_string())
      .collect::<Vec<String>>();
    self
      .connection_manager
      .xack(&self.stream_key, &self.group_name, &message_ids)
      .await?;
    Ok(())
  }

  /// Acknowledges messages processed by a consumer.
  ///
  /// In Redis streams, when a message is delivered to a consumer using XREADGROUP, it moves into
  /// a pending state for that consumer. Redis expects you to manually acknowledge these messages
  /// using XACK once they have been successfully processed. If you don't acknowledge a message,
  /// it remains in the pending state for that consumer. Redis keeps track of these messages so you
  /// can handle message failures or retries.
  pub async fn ack_messages(&mut self, messages: &[StreamMessage]) -> Result<(), StreamError> {
    if messages.is_empty() {
      return Ok(());
    }

    let message_ids = messages
      .iter()
      .map(|m| m.id.to_string())
      .collect::<Vec<String>>();
    self.ack_message_ids(message_ids).await
  }

  /// Inserts multiple messages into the Redis stream
  /// the order of messages submitted through a pipeline is guaranteed to be executed in the order
  /// they are added to the pipeline.
  ///
  /// Pipelining is effective for inserting multiple messages as it significantly reduces the latency
  /// associated with multiple independent network requests.
  pub async fn insert_messages<T: Into<StreamBinary>>(
    &mut self,
    messages: Vec<T>,
  ) -> Result<(), StreamError> {
    if messages.is_empty() {
      return Ok(());
    }

    let mut pipe = pipe();
    for message in messages {
      let message = message.into();
      let tuple = message.into_tuple_array();
      pipe.xadd(&self.stream_key, "*", tuple.as_slice());
    }
    pipe.query_async(&mut self.connection_manager).await?;
    Ok(())
  }

  /// Inserts a single message into the Redis stream.
  pub async fn insert_message<T: TryInto<StreamBinary, Error = StreamError>>(
    &mut self,
    message: T,
  ) -> Result<(), StreamError> {
    let message = message.try_into()?;
    self.insert_binary(message).await
  }

  pub async fn insert_binary(&mut self, message: StreamBinary) -> Result<(), StreamError> {
    let tuple = message.into_tuple_array();
    self
      .connection_manager
      .xadd(&self.stream_key, "*", tuple.as_slice())
      .await?;
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
    option: ReadOption,
  ) -> Result<Vec<StreamMessage>, StreamError> {
    let mut options = StreamReadOptions::default()
      .group(&self.group_name, consumer_name)
      .block(100);

    let message_id;
    match option {
      ReadOption::Undelivered => {
        message_id = ">".to_string();
      },
      ReadOption::Count(count) => {
        message_id = ">".to_string();
        options = options.count(count);
      },
      ReadOption::After(after) => {
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
  pub async fn get_unacked_messages(
    &mut self,
    consumer_name: &str,
  ) -> Result<Vec<StreamMessage>, StreamError> {
    let pending = self.get_pending().await?;

    match pending {
      None => Ok(vec![]),
      Some(pending) => {
        let messages = self
          .get_unacked_messages_with_range(consumer_name, &pending.start_id, &pending.end_id)
          .await?;
        Ok(messages)
      },
    }
  }

  /// Get unacknowledged messages
  ///
  /// `min_idle_time` indicates the minimum amount of time a message should have been idle
  /// (i.e., not acknowledged) before it can be claimed by another consumer. "Idle" time is
  /// essentially how long the message has been unacknowledged since its last delivery to any consumer.
  ///
  pub async fn get_unacked_messages_with_range(
    &mut self,
    consumer_name: &str,
    start_id: &str,
    end_id: &str,
  ) -> Result<Vec<StreamMessage>, StreamError> {
    let opts = StreamClaimOptions::default()
      .idle(500)
      .with_force()
      .retry(2);

    // If the start_id and end_id are the same, we only need to claim one message.
    let mut ids = Vec::with_capacity(2);
    ids.push(start_id);
    if start_id != end_id {
      ids.push(end_id);
    }

    let result: StreamClaimReply = self
      .connection_manager
      .xclaim_options(
        &self.stream_key,
        &self.group_name,
        consumer_name,
        500,
        &ids,
        opts,
      )
      .await?;

    let mut messages = vec![];
    trace!("Claimed messages: {}", result.ids.len());
    for id in result.ids {
      match StreamMessage::try_from(id) {
        Ok(message) => messages.push(message),
        Err(err) => {
          error!("{:?}", err);
        },
      }
    }
    Ok(messages)
  }

  /// Reads all messages from the stream
  ///
  pub async fn get_all_message(&mut self) -> Result<Vec<StreamMessage>, StreamError> {
    let read_messages: Vec<StreamMessage> =
      self.connection_manager.xrange_all(&self.stream_key).await?;
    Ok(read_messages.into_iter().collect())
  }

  pub async fn get_pending(&mut self) -> Result<Option<StreamPendingData>, StreamError> {
    let reply: StreamPendingReply = self
      .connection_manager
      .xpending(&self.stream_key, &self.group_name)
      .await?;

    match reply {
      StreamPendingReply::Empty => Ok(None),
      StreamPendingReply::Data(data) => Ok(Some(data)),
    }
  }

  /// Clears all messages from the specified Redis stream.
  ///
  /// Use the `XTRIM` command to truncate the Redis stream to a maximum length of zero, effectively
  /// removing all entries from the stream.
  pub async fn clear(&mut self) -> Result<(), StreamError> {
    self
      .connection_manager
      .xtrim(&self.stream_key, StreamMaxlen::Equals(0))
      .await?;
    Ok(())
  }
}

pub enum ReadOption {
  Undelivered,
  Count(usize),
  After(MessageId),
}
