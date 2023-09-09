use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::error::AppError;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Error, Default, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(i32)]
pub enum ErrorCode {
  #[error("Operation completed successfully.")]
  #[default]
  Ok = 0,

  #[error("An unhandled error occurred.")]
  Unhandled = -1,

  #[error("Invalid email format.")]
  InvalidEmail = 1001,

  #[error("Invalid password.")]
  InvalidPassword = 1002,

  #[error("OAuth authentication error.")]
  OAuthError = 1003,
}

impl From<anyhow::Error> for ErrorCode {
  fn from(err: anyhow::Error) -> Self {
    err.downcast::<ErrorCode>().unwrap_or(ErrorCode::Unhandled)
  }
}

pub fn invalid_email_error(email: &str) -> AppError {
  AppError::new(ErrorCode::InvalidEmail, format!("invalid email: {}", email))
}

pub fn invalid_password_error(password: &str) -> AppError {
  AppError::new(
    ErrorCode::InvalidPassword,
    format!("invalid password: {}", password),
  )
}
