use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::error::AppError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(i64)]
pub enum ErrorCode {
  Ok = 0,
  Unhandled = -1,

  InvalidEmail = 1001,
  InvalidPassword = 1002,
  OAuthError = 1003,
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
