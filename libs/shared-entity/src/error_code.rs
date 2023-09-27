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

  #[error("Record already exist")]
  RecordAlreadyExists = -3,

  #[error("Invalid email format.")]
  InvalidEmail = 1001,

  #[error("Invalid password.")]
  InvalidPassword = 1002,

  #[error("OAuth authentication error.")]
  OAuthError = 1003,

  #[error("Missing Payload")]
  MissingPayload = 1004,

  #[error("Storage error")]
  StorageError = 1005,

  #[error("Open Error")]
  OpenError = 1006,

  #[error("Invalid Url")]
  InvalidUrl = 1007,

  #[error("Invalid Request Parameters")]
  InvalidRequestParams = 1008,

  #[error("Url Missing Parameter")]
  UrlMissingParameter = 1009,

  #[error("Invalid OAuth Provider")]
  InvalidOAuthProvider = 1010,

  #[error("Not Logged In")]
  NotLoggedIn = 1011,

  #[error("Not Enough Permissions")]
  NotEnoughPermissions = 1012,

  #[error("User name cannot be empty")]
  UserNameIsEmpty = 1013,

  #[error("S3 Error")]
  S3Error = 1014,

  #[error("File Not Found")]
  FileNotFound = 1015,
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

pub fn url_missing_param(param: &str) -> AppError {
  AppError::new(
    ErrorCode::UrlMissingParameter,
    format!("url missing parameter: {}", param),
  )
}
