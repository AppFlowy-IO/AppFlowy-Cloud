use reqwest::StatusCode;
use tokio_tungstenite::tungstenite::Error;

#[derive(Debug, thiserror::Error)]
pub enum WSError {
  #[error("Unsupported ws message type")]
  UnsupportedMsgType,

  #[error(transparent)]
  TungsteniteError(Error),

  #[error("Auth error: {0}")]
  AuthError(String),

  #[error(transparent)]
  SerdeError(#[from] serde_json::Error),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

impl From<Error> for WSError {
  fn from(value: Error) -> Self {
    if let Error::Http(resp) = &value {
      if resp.status() == StatusCode::UNAUTHORIZED {
        return WSError::AuthError("Unauthorized websocket connection".to_string());
      }
    }
    WSError::TungsteniteError(value)
  }
}
