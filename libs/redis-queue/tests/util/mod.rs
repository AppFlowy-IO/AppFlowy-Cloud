use anyhow::{Context, Error};
use redis_queue::message::{RedisMessageDecode, RedisMessageEncode};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TestRedisQueueMessage {
  pub id: String,
}

impl RedisMessageEncode for TestRedisQueueMessage {
  fn encode_message(&self) -> Result<Vec<u8>, Error> {
    Ok(self.id.as_bytes().to_vec())
  }
}

impl RedisMessageDecode for TestRedisQueueMessage {
  fn decode_message(payload: &[u8]) -> Result<Self, Error>
  where
    Self: Sized,
  {
    Ok(TestRedisQueueMessage {
      id: String::from_utf8(payload.to_vec())?,
    })
  }
}

pub async fn redis_client() -> redis::Client {
  let redis_uri = "redis://localhost:6379";
  redis::Client::open(redis_uri)
    .context("failed to connect to redis")
    .unwrap()
}
