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
      "gotrue error: {} code: {}, error_id: {:?}",
      self.msg, self.code, self.error_id
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

impl From<OAuthError> for GoTrueError {
  fn from(value: OAuthError) -> Self {
    GoTrueError {
      code: 400,
      msg: format!(
        "oauth error: {}, description: {}",
        value.error,
        value.error_description.unwrap_or_default(),
      ),
      error_id: None,
    }
  }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OAuthError {
  pub error: String,
  pub error_description: Option<String>,
}

impl Display for OAuthError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self.error_description {
      Some(ref desc) => write!(f, "{}: {}", self.error, desc),
      None => write!(f, "{}", self.error),
    }
  }
}
