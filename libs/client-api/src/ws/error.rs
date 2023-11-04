use crate::ws::ClientRealtimeMessage;
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
  SenderError(#[from] tokio::sync::broadcast::error::SendError<ClientRealtimeMessage>),

  #[error("Internal failure: {0}")]
  Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
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
