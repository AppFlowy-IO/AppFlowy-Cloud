#[derive(Debug, thiserror::Error)]
pub enum AIError {
  #[error(transparent)]
  Internal(#[from] anyhow::Error),

  #[error("Request timeout:{0}")]
  RequestTimeout(String),

  #[error("Payload too large:{0}")]
  PayloadTooLarge(String),

  #[error("Invalid request:{0}")]
  InvalidRequest(String),
}
