use redis::RedisError;

#[derive(thiserror::Error, Debug)]
pub enum StreamError {
  #[error(transparent)]
  RedisError(#[from] RedisError),

  #[error("Stream already exist: {0}")]
  StreamAlreadyExist(String),

  #[error("Stream not exist: {0}")]
  StreamNotExist(String),

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

  #[error("failed to decode update: {0}")]
  UpdateError(#[from] collab::preclude::encoding::read::Error),

  #[error("I/O error: {0}")]
  IO(#[from] std::io::Error),

  #[error("Internal error: {0}")]
  Internal(anyhow::Error),
}

impl StreamError {
  pub fn is_stream_not_exist(&self) -> bool {
    matches!(self, StreamError::StreamNotExist(_))
  }
}

pub fn internal<T: ToString>(msg: T) -> RedisError {
  let msg = msg.to_string();
  RedisError::from((redis::ErrorKind::TypeError, "", msg))
}
