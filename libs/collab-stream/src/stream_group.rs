use crate::error::StreamError;
use crate::model::{MessageId, StreamBinary, StreamMessage, StreamMessageByStreamKey};
use chrono::{DateTime, Utc};
use redis::aio::ConnectionManager;
use redis::streams::{
  StreamClaimOptions, StreamClaimReply, StreamMaxlen, StreamPendingData, StreamPendingReply,
  StreamReadOptions,
};
use redis::{pipe, AsyncCommands, RedisResult};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use tracing::{error, trace, warn};

#[derive(Clone)]
pub struct StreamGroup {
  connection_manager: ConnectionManager,
  stream_key: String,
  group_name: String,
  config: StreamConfig,
  last_expiration_set: Option<DateTime<Utc>>,
  cancel_token: Arc<CancellationToken>,
}
impl Drop for StreamGroup {
  fn drop(&mut self) {
    self.cancel_token.cancel();
  }
}
impl StreamGroup {
  pub fn new(stream_key: String, group_name: &str, connection_manager: ConnectionManager) -> Self {
    let config = StreamConfig {
      max_len: Some(1000),
      expire_time_in_secs: None,
    };
    Self::new_with_config(stream_key, group_name, connection_manager, config)
  }

  pub fn new_with_config(
    stream_key: String,
    group_name: &str,
    connection_manager: ConnectionManager,
    config: StreamConfig,
  ) -> Self {
    let cancel_token = Arc::new(CancellationToken::new());
    let group_name = group_name.to_string();
    let group = Self {
      group_name,
      connection_manager,
      stream_key,
      config,
      last_expiration_set: None,
      cancel_token,
    };
    group.spawn_periodic_check();
    group
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
      if let Some(len) = self.config.max_len {
        pipe
          .cmd("XADD")
          .arg(&self.stream_key)
          .arg("MAXLEN")
          .arg("~")
          .arg(len)
          .arg("*")
          .arg(&tuple);
      } else {
        pipe.cmd("XADD").arg(&self.stream_key).arg("*").arg(&tuple);
      }
    }

    pipe.query_async(&mut self.connection_manager).await?;
    if let Err(err) = self.set_expiration().await {
      error!("set expiration fail: {:?}", err);
    }
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
    match self.config.max_len {
      Some(max_len) => {
        self
          .connection_manager
          .xadd_maxlen(&self.stream_key, StreamMaxlen::Approx(max_len), "*", &tuple)
          .await?;
      },
      None => {
        self
          .connection_manager
          .xadd(&self.stream_key, "*", tuple.as_slice())
          .await?;
      },
    }

    if let Err(err) = self.set_expiration().await {
      error!("set expiration fail: {:?}", err);
    }
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

  /// For now, we only call this function after inserting messages into the stream.
  ///
  /// Inserting a Redis EXPIRE command after adding messages ensures that the stream's
  /// TTL (time-to-live) is updated whenever new data is inserted. This is crucial for streams
  /// where data needs to be automatically cleaned up after a certain period. Setting the expiration
  /// after every insertion guarantees that the stream will expire correctly, even if messages are
  /// inserted at irregular intervals.
  async fn set_expiration(&mut self) -> Result<(), StreamError> {
    let now = Utc::now();
    if let Some(expire_time) = self.config.expire_time_in_secs {
      let should_set_expiration = match self.last_expiration_set {
        // Set expiration if it has been more than an hour since the last set
        Some(last_set) => now.signed_duration_since(last_set) > chrono::Duration::seconds(60 * 60),
        None => true,
      };

      if should_set_expiration {
        self
          .connection_manager
          .expire(&self.stream_key, expire_time)
          .await?;
        self.last_expiration_set = Some(now);
      }
    }

    Ok(())
  }

  /// Spawns a periodic task to check the stream length.
  fn spawn_periodic_check(&self) {
    if let Some(max_len) = self.config.max_len {
      let stream_key = self.stream_key.clone();
      let mut connection_manager = self.connection_manager.clone();
      let cancel_token = self.cancel_token.clone();
      tokio::spawn(async move {
        // Check every hour
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        loop {
          tokio::select! {
              _ = interval.tick() => {
                if let Ok(len) = get_stream_length(&mut connection_manager, &stream_key).await {
                  if len + 100 > max_len {
                    warn!("stream len is going to exceed the max len: {}, current: {}", max_len, len);
                  }
                }
              }
              _ = cancel_token.cancelled() => {
                trace!("Stream length check task cancelled.");
                break;
              }
          }
        }
      });
    }
  }
}

/// Checks if the stream length exceeds the maximum length.
async fn get_stream_length(
  connection_manager: &mut ConnectionManager,
  stream_key: &str,
) -> Result<usize, StreamError> {
  let current_len: usize = connection_manager.xlen(stream_key).await?;
  Ok(current_len)
}

pub enum ReadOption {
  Undelivered,
  Count(usize),
  After(MessageId),
}

#[derive(Clone)]
pub struct StreamConfig {
  /// Sets the maximum length of the stream.
  max_len: Option<usize>,
  /// Set the time in secs for the stream to expire. After the time has passed, the stream will be
  /// automatically deleted. All messages in the stream will be removed.
  ///
  /// If the stream does not exist (e.g., it has expired), inserting a message will automatically
  /// create the stream.
  expire_time_in_secs: Option<i64>,
}

impl Default for StreamConfig {
  fn default() -> Self {
    Self::new()
  }
}

impl StreamConfig {
  pub fn new() -> Self {
    Self {
      max_len: None,
      expire_time_in_secs: None,
    }
  }
  pub fn with_max_len(mut self, max_len: usize) -> Self {
    self.max_len = Some(max_len);
    self
  }

  pub fn with_expire_time(mut self, expire_time_in_secs: i64) -> Self {
    self.expire_time_in_secs = Some(expire_time_in_secs);
    self
  }
}
