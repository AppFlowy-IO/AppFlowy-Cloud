use anyhow::Error;

pub trait RedisMessageEncode {
  fn encode_message(&self) -> Result<Vec<u8>, Error>;
}

pub trait RedisMessageDecode {
  fn decode_message(payload: &[u8]) -> Result<Self, Error>
  where
    Self: Sized;
}
