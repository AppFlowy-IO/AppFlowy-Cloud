use crate::message::RedisMessageEncode;
use anyhow::{anyhow, Error};
use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use tokio::sync::Mutex;

pub struct Producer {
  queue_name: String,
  client: Mutex<MultiplexedConnection>,
}

impl Producer {
  pub fn new(queue_name: String, client: MultiplexedConnection) -> Producer {
    Producer {
      queue_name,
      client: Mutex::new(client),
    }
  }

  pub async fn push<T: RedisMessageEncode>(&self, message: T) -> Result<(), Error> {
    let encoded = message.encode_message()?;
    self
      .client
      .lock()
      .await
      .lpush(self.queue_name.as_str(), encoded)
      .await
      .or(Err(anyhow!("failed to push")))
  }
}
