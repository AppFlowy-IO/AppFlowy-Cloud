use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Serialize, Deserialize, Debug)]
pub struct GoTrueError {
  pub code: i64,
  pub msg: String,
  pub error_id: Option<String>,
}

impl std::error::Error for GoTrueError {}

impl Display for GoTrueError {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "code: {}, msg:{}, error_id: {:?}",
      self.code, self.msg, self.error_id
    ))
  }
}

impl From<anyhow::Error> for GoTrueError {
  fn from(value: anyhow::Error) -> Self {
    GoTrueError {
      code: -1,
      msg: format!("gotrue unhandled error: {}", value),
      error_id: None,
    }
  }
}

impl From<reqwest::Error> for GoTrueError {
  fn from(value: reqwest::Error) -> Self {
    GoTrueError {
      code: -1,
      msg: format!("gotrue reqwest error: {}", value),
      error_id: None,
    }
  }
}

impl From<GotrueClientError> for GoTrueError {
  fn from(value: GotrueClientError) -> Self {
    GoTrueError {
      code: 400,
      msg: format!(
        "gotrue client error: {}, description: {}",
        value.error,
        value.error_description.unwrap_or_default(),
      ),
      error_id: None,
    }
  }
}

/// Used to deserialize the response from the gotrue server
#[derive(Serialize, Deserialize, Debug)]
pub struct GotrueClientError {
  pub error: String,
  pub error_description: Option<String>,
}
