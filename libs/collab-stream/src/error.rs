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

  #[error("Internal error: {0}")]
  Internal(anyhow::Error),
}

pub fn parse_error<T>(value: &redis::Value) -> RedisError {
  RedisError::from((
    redis::ErrorKind::TypeError,
    "unexpected value",
    format!("can't parse {:?} to {}", value, std::any::type_name::<T>()),
  ))
}

pub fn internal<T: ToString>(msg: T) -> RedisError {
  let msg = msg.to_string();
  RedisError::from((redis::ErrorKind::TypeError, "", msg))
}
