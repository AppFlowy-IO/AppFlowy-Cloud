use redis::RedisError;

#[derive(thiserror::Error, Debug)]
pub enum StreamError {
  #[error(transparent)]
  RedisError(#[from] RedisError),

  #[error("Unexpected value: {0}")]
  UnexpectedValue(String),

  #[error(transparent)]
  Utf8Error(#[from] std::str::Utf8Error),

  #[error("Invalid format")]
  InvalidFormat,

  #[error(transparent)]
  ParseIntError(#[from] std::num::ParseIntError),

  #[error("Stream group already exists")]
  GroupAlreadyExists(String),

  #[error(transparent)]
  SerdeJsonError(#[from] serde_json::Error),

  #[error(transparent)]
  BinCodeSerde(#[from] bincode::Error),

  #[error("Internal error: {0}")]
  Internal(anyhow::Error),
}

pub fn internal<T: ToString>(msg: T) -> RedisError {
  let msg = msg.to_string();
  RedisError::from((redis::ErrorKind::TypeError, "", msg))
}
