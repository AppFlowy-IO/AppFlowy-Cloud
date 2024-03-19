use crate::message::RedisMessageDecode;
use anyhow::{anyhow, Error};
use redis::aio::MultiplexedConnection;
use redis::{AsyncCommands, RedisResult, Value};
use std::ops::{Deref, DerefMut};

use std::sync::atomic::AtomicBool;

use tokio::sync::Mutex;

pub const CONSUMERS_KEY: &str = "consumers";
pub const PROCESSING_QUEUE_KEY: &str = "consumers:{consumer}:processing";
pub const UNACK_QUEUE_KEY: &str = "consumers:{consumer}:unack";
pub struct Consumer {
  /// The name of the consumer.
  pub name: String,
  /// The main queue from which the consumer reads messages
  pub queue: String,
  /// A queue for messages that are currently being processed
  processing_queue: String,
  /// A queue for messages that were processed but not acknowledged.
  unack_queue: String,
  client: Mutex<MultiplexedConnection>,
  stopped: AtomicBool,
}

impl Consumer {
  /// Constructs a new `Consumer`.
  ///
  /// This function initializes a new consumer with the provided name, queue, and Redis connection.
  /// It also prepares specific Redis keys for managing the processing and acknowledgment of messages.
  ///
  /// # Parameters
  /// - `name`: The unique name of the consumer. This name is used to generate specific Redis keys
  ///   related to the consumer's activity, processing queue, and unacknowledged
  ///   messages queue.
  /// - `queue`: The name of the main queue from which the consumer will retrieve messages.
  /// - `client`: An active connection to the Redis server. This connection is used for all Redis
  ///   operations performed by the consumer.
  ///
  /// # Returns
  /// Returns a new instance of `Consumer` configured with the provided name, queue, and Redis
  /// connection. The consumer is ready to start processing messages from the specified queue.
  ///
  pub fn new(name: String, queue: String, client: MultiplexedConnection) -> Consumer {
    let processing_queue = PROCESSING_QUEUE_KEY.replace("{consumer}", name.as_str());
    let unack_queue = UNACK_QUEUE_KEY.replace("{consumer}", name.as_str());
    let stopped = AtomicBool::new(false);
    Consumer {
      name,
      queue,
      processing_queue,
      unack_queue,
      client: Mutex::new(client),
      stopped,
    }
  }

  pub fn stop(&self) {
    self
      .stopped
      .store(true, std::sync::atomic::Ordering::SeqCst);
  }

  pub fn is_stopped(&self) -> bool {
    self.stopped.load(std::sync::atomic::Ordering::SeqCst)
  }

  pub async fn message_len(&self) -> RedisResult<u64> {
    self.client.lock().await.llen(&self.queue).await
  }

  pub async fn next<T: RedisMessageDecode>(&self) -> Result<NextMessage<T>, Error> {
    if self.is_stopped() {
      return Err(anyhow!("consumer is stopped"));
    }

    let value: Value = self
      .client
      .lock()
      .await
      .brpoplpush(&self.queue, &self.processing_queue, 6f64)
      .await?;

    match value {
      Value::Data(data) => {
        let message = T::decode_message(&data)?;
        Ok(NextMessage::new(
          message,
          data,
          &self.client,
          self.processing_queue.clone(),
          self.unack_queue.clone(),
        ))
      },
      _ => Err(anyhow!("unexpected value type: {:?}", value)),
    }
  }
}

#[derive(Debug)]
pub struct NextMessage<'a, T: 'a> {
  message: T,
  payload: Vec<u8>,
  client: &'a Mutex<MultiplexedConnection>,
  processing_queue: String,
  unack_queue: String,
}

impl<'a, T> Deref for NextMessage<'a, T> {
  type Target = T;

  fn deref(&self) -> &T {
    &self.message
  }
}

impl<'a, T> NextMessage<'a, T> {
  pub fn new(
    message: T,
    payload: Vec<u8>,
    client: &'a Mutex<MultiplexedConnection>,
    processing_queue: String,
    unack_queue: String,
  ) -> NextMessage<'a, T> {
    NextMessage {
      message,
      payload,
      client,
      processing_queue,
      unack_queue,
    }
  }

  pub fn payload(&self) -> &[u8] {
    &self.payload
  }

  pub fn message(&self) -> &T {
    &self.message
  }

  pub fn into_message(self) -> T {
    self.message
  }

  /// Acknowledges the message, removing it from the processing queue.
  pub async fn ack(&mut self) -> Result<Value, Error> {
    let value = self
      .client
      .lock()
      .await
      .lrem(self.processing_queue.as_str(), 1, self.payload.clone())
      .await?;
    Ok(value)
  }

  /// Rejects the message, moving it to the unacknowledged queue.
  pub async fn reject(&mut self) -> Result<Value, Error> {
    let value = self.push(self.unack_queue.clone()).await?;
    Ok(value)
  }

  /// Utility method to move the message to a specified queue and remove it from the processing queue.
  pub async fn push(&mut self, push_queue_name: String) -> Result<Value, Error> {
    let value = redis::pipe()
      .atomic()
      .cmd("LPUSH")
      .arg(push_queue_name.as_str())
      .arg(self.payload.clone())
      .ignore()
      .cmd("LREM")
      .arg(self.processing_queue.as_str())
      .arg(1)
      .arg(self.payload.clone())
      .ignore()
      .query_async(self.client.lock().await.deref_mut())
      .await?;
    Ok(value)
  }
}
