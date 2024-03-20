use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GoTrueError {
  #[error("connect error:{0}")]
  Connect(String),

  #[error("request timeout:{0}")]
  RequestTimeout(String),

  #[error("invalid request:{0}")]
  InvalidRequest(String),

  #[error(transparent)]
  ClientError(#[from] GotrueClientError),

  #[error(transparent)]
  Internal(#[from] GoTrueErrorSerde),

  #[error("{0}")]
  NotLoggedIn(String),

  #[error("{0}")]
  Auth(String),

  #[error(transparent)]
  Unhandled(#[from] anyhow::Error),
}

impl GoTrueError {
  pub fn is_network_error(&self) -> bool {
    matches!(
      self,
      GoTrueError::Connect(_) | GoTrueError::RequestTimeout(_)
    )
  }
}

impl From<reqwest::Error> for GoTrueError {
  fn from(value: reqwest::Error) -> Self {
    #[cfg(not(target_arch = "wasm32"))]
    if value.is_connect() {
      return GoTrueError::Connect(value.to_string());
    }

    if value.is_timeout() {
      return GoTrueError::RequestTimeout(value.to_string());
    }

    if value.is_request() {
      return GoTrueError::InvalidRequest(value.to_string());
    }

    if let Some(status) = value.status() {
      if status == StatusCode::UNAUTHORIZED {
        return GoTrueError::Auth(value.to_string());
      }
    }

    GoTrueError::Unhandled(value.into())
  }
}

#[derive(Serialize, Deserialize, Debug, Error)]
pub struct GoTrueErrorSerde {
  pub code: i64,
  pub msg: String,
  pub error_id: Option<String>,
}

impl Display for GoTrueErrorSerde {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "code: {}, msg:{}, error_id: {:?}",
      self.code, self.msg, self.error_id
    ))
  }
}

/// The gotrue error definition:
/// https://github.com/supabase/gotrue/blob/cc07b4aa2ace75d9c8e46ae5107dbabadf944e87/internal/models/errors.go#L65
/// Used to deserialize the response from the gotrue server
#[derive(Serialize, Deserialize, Debug, Error)]
pub struct GotrueClientError {
  pub error: String,
  pub error_description: Option<String>,
}

impl Display for GotrueClientError {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "error: {}, description: {:?}",
      self.error, self.error_description
    ))
  }
}
