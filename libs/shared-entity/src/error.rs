use std::borrow::Cow;
use std::fmt::Display;

use crate::server_error::ErrorCode;
use actix_web::{http::StatusCode, HttpResponse};
use gotrue::models::{GoTrueError, OAuthError};
use serde::{Deserialize, Serialize};
use serde_json::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppError {
  pub code: ErrorCode,
  pub message: Cow<'static, str>,
}

impl AppError {
  pub fn new(code: ErrorCode, message: impl Into<Cow<'static, str>>) -> Self {
    Self {
      code,
      message: message.into(),
    }
  }
}

impl Display for AppError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.message)
  }
}

impl std::error::Error for AppError {}

//
impl actix_web::error::ResponseError for AppError {
  fn status_code(&self) -> StatusCode {
    StatusCode::OK
  }

  fn error_response(&self) -> HttpResponse {
    HttpResponse::Ok().json(self)
  }
}
//
impl From<anyhow::Error> for AppError {
  fn from(err: anyhow::Error) -> Self {
    match err.downcast_ref::<AppError>() {
      None => AppError::new(ErrorCode::Unhandled, err.to_string()),
      Some(err) => err.clone(),
    }
  }
}

impl From<GoTrueError> for AppError {
  fn from(err: GoTrueError) -> Self {
    AppError::new(
      ErrorCode::Unhandled,
      format!(
        "gotrue error: {}, id: {}",
        err.code,
        err.error_id.unwrap_or("".to_string())
      ),
    )
  }
}

impl From<OAuthError> for AppError {
  fn from(err: OAuthError) -> Self {
    AppError::new(ErrorCode::OAuthError, err.to_string())
  }
}

impl From<ErrorCode> for AppError {
  fn from(value: ErrorCode) -> Self {
    AppError::new(value, value.to_string())
  }
}

impl From<reqwest::Error> for AppError {
  fn from(value: reqwest::Error) -> Self {
    AppError::new(ErrorCode::Unhandled, value.to_string())
  }
}

impl From<serde_json::Error> for AppError {
  fn from(value: Error) -> Self {
    AppError::new(ErrorCode::Unhandled, value.to_string())
  }
}
