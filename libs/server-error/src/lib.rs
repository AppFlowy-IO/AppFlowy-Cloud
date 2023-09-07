use std::fmt::Display;

use actix_web::{http::StatusCode, HttpResponse};
use gotrue::models::GoTrueError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Error {
  pub message: String,
  pub code: i64,
}

impl Display for Error {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.message)
  }
}

impl actix_web::error::ResponseError for Error {
  fn status_code(&self) -> StatusCode {
    StatusCode::OK
  }

  fn error_response(&self) -> HttpResponse {
    serde_json::to_string(self)
      .map(|json| HttpResponse::build(StatusCode::OK).body(json))
      .unwrap_or_else(|e| {
        HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).body(e.to_string())
      })
  }
}

impl From<anyhow::Error> for Error {
  fn from(err: anyhow::Error) -> Self {
    Self {
      message: format!("unhandled error: {}", err),
      code: -1,
    }
  }
}

impl From<GoTrueError> for Error {
  fn from(err: GoTrueError) -> Self {
    Self {
      message: format!(
        "gotrue error: {}, id: {}",
        err.code,
        err.error_id.unwrap_or("".to_string())
      ),
      code: err.code,
    }
  }
}
