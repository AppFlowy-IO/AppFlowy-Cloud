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

  #[error("Record not found")]
  RecordNotFound = -2,

  #[error("Invalid email format.")]
  InvalidEmail = 1001,

  #[error("Invalid password.")]
  InvalidPassword = 1002,

  #[error("OAuth authentication error.")]
  OAuthError = 1003,

  #[error("Missing Payload")]
  MissingPayload = 1004,
}

/// Implements conversion from `anyhow::Error` to `ErrorCode`.
///
/// This implementation checks if the `anyhow::Error` can be downcasted
/// to an `AppError` or `ErrorCode`. If the error is of type `AppError`,
/// it returns the associated error code. If the error can be downcasted
/// to `ErrorCode`, it returns the error code directly. Otherwise, it
/// defaults to `ErrorCode::Unhandled`.
impl From<anyhow::Error> for ErrorCode {
  fn from(err: anyhow::Error) -> Self {
    if let Some(err) = err.downcast_ref::<AppError>() {
      return err.code;
    }

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
