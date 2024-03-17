use client_websocket::{Error, ProtocolError};
use reqwest::StatusCode;

#[derive(Debug, thiserror::Error)]
pub enum WSError {
  #[error(transparent)]
  TungsteniteError(Error),

  #[error("{0}")]
  LostConnection(String),

  #[error("{0}")]
  Close(String),

  #[error("Auth error: {0}")]
  AuthError(String),

  #[error("Fail to send message via http: {0}")]
  Http(String),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

impl WSError {
  pub fn should_stop(&self) -> bool {
    matches!(self, WSError::LostConnection(_) | WSError::Close(_))
  }
}

impl From<Error> for WSError {
  fn from(value: Error) -> Self {
    match &value {
      Error::ConnectionClosed => WSError::LostConnection(value.to_string()),
      Error::AlreadyClosed => WSError::Close(value.to_string()),
      Error::Protocol(ProtocolError::SendAfterClosing) => WSError::Close(value.to_string()),
      Error::Http(resp) => {
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::NOT_FOUND {
          WSError::AuthError("Unauthorized websocket connection".to_string())
        } else {
          WSError::TungsteniteError(value)
        }
      },
      _ => WSError::TungsteniteError(value),
    }
  }
}
