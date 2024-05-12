use reqwest::StatusCode;

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

impl From<reqwest::Error> for AIError {
  fn from(error: reqwest::Error) -> Self {
    if error.is_timeout() {
      return AIError::RequestTimeout(error.to_string());
    }

    if error.is_request() {
      return if error.status() == Some(StatusCode::PAYLOAD_TOO_LARGE) {
        AIError::PayloadTooLarge(error.to_string())
      } else {
        AIError::InvalidRequest(format!("{:?}", error))
      };
    }
    AIError::Internal(error.into())
  }
}
