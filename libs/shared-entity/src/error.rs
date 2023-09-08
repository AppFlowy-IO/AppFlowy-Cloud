use std::fmt::Display;

use crate::{data::AppData, server_error::ErrorCode};
use actix_web::{http::StatusCode, HttpResponse};
use gotrue::models::GoTrueError;

pub type AppError = AppData<()>;

impl AppError {
  pub fn new(code: ErrorCode, message: String) -> Self {
    Self {
      code,
      message,
      data: (),
    }
  }
}

impl Display for AppError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.message)
  }
}

impl actix_web::error::ResponseError for AppError {
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

impl From<anyhow::Error> for AppError {
  fn from(err: anyhow::Error) -> Self {
    AppError::new(ErrorCode::Unhandled, format!("unhandled error: {}", err))
  }
}

impl From<GoTrueError> for AppError {
  fn from(err: GoTrueError) -> Self {
    match err.code {
      // Example:
      // 404 => AppError::new(
      // ...
      // ),
      _ => AppError::new(
        ErrorCode::Unhandled,
        format!(
          "gotrue error: {}, id: {}",
          err.code,
          err.error_id.unwrap_or("".to_string())
        ),
      ),
    }
  }
}
