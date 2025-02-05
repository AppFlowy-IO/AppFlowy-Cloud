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

  #[error(transparent)]
  SerdeError(#[from] serde_json::Error),

  #[error("Service unavailable:{0}")]
  ServiceUnavailable(String),
}
