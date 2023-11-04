#[cfg(feature = "gotrue_error")]
pub mod gotrue;

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Default)]
pub enum AppError {
  #[error("Operation completed successfully.")]
  #[default]
  Ok,

  #[error(transparent)]
  Internal(#[from] anyhow::Error),

  #[error("An unhandled error occurred:{0}")]
  Unhandled(String),

  #[error("Record not found:{0}")]
  RecordNotFound(String),

  #[error("Record already exist:{0}")]
  RecordAlreadyExists(String),

  #[error("Invalid email format:{0}")]
  InvalidEmail(String),

  #[error("Invalid password:{0}")]
  InvalidPassword(String),

  #[error("OAuth error:{0}")]
  OAuthError(String),

  #[error("Missing Payload:{0}")]
  MissingPayload(String),

  #[error("Database error:{0}")]
  DBError(String),

  #[error("Open Error:{0}")]
  OpenError(String),

  #[error("Invalid Url:{0}")]
  InvalidUrl(String),

  #[error("Invalid parameters:{0}")]
  InvalidRequestParams(String),

  #[error("Url Missing Parameter:{0}")]
  UrlMissingParameter(String),

  #[error("Invalid OAuth Provider:{0}")]
  InvalidOAuthProvider(String),

  #[error("Not Logged In:{0}")]
  NotLoggedIn(String),

  #[error("Not Enough Permissions:{0}")]
  NotEnoughPermissions(String),

  #[cfg(feature = "s3_error")]
  #[error(transparent)]
  S3Error(#[from] s3::error::S3Error),

  #[error("s3 response error:{0}")]
  S3ResponseError(String),

  #[error("Storage space not enough")]
  StorageSpaceNotEnough,

  #[error("Payload too large:{0}")]
  PayloadTooLarge(String),

  #[error(transparent)]
  UuidError(#[from] uuid::Error),

  #[error(transparent)]
  IOError(#[from] std::io::Error),

  #[cfg(feature = "sqlx_error")]
  #[error("{0}")]
  SqlxError(String),

  #[cfg(feature = "validation_error")]
  #[error(transparent)]
  ValidatorError(#[from] validator::ValidationErrors),

  #[error(transparent)]
  UrlError(#[from] url::ParseError),

  #[error(transparent)]
  SerdeError(#[from] serde_json::Error),

  #[error(transparent)]
  ReqwestError(#[from] reqwest::Error),
}

impl AppError {
  pub fn is_record_not_found(&self) -> bool {
    matches!(self, AppError::RecordNotFound(_))
  }

  pub fn code(&self) -> ErrorCode {
    match self {
      AppError::Ok => ErrorCode::Ok,
      AppError::Unhandled(_) => ErrorCode::Unhandled,
      AppError::RecordNotFound(_) => ErrorCode::RecordNotFound,
      AppError::RecordAlreadyExists(_) => ErrorCode::RecordAlreadyExists,
      AppError::InvalidEmail(_) => ErrorCode::InvalidEmail,
      AppError::InvalidPassword(_) => ErrorCode::InvalidPassword,
      AppError::OAuthError(_) => ErrorCode::OAuthError,
      AppError::MissingPayload(_) => ErrorCode::MissingPayload,
      AppError::DBError(_) => ErrorCode::DBError,
      AppError::OpenError(_) => ErrorCode::OpenError,
      AppError::InvalidUrl(_) => ErrorCode::InvalidUrl,
      AppError::InvalidRequestParams(_) => ErrorCode::InvalidRequestParams,
      AppError::UrlMissingParameter(_) => ErrorCode::UrlMissingParameter,
      AppError::InvalidOAuthProvider(_) => ErrorCode::InvalidOAuthProvider,
      AppError::NotLoggedIn(_) => ErrorCode::NotLoggedIn,
      AppError::NotEnoughPermissions(_) => ErrorCode::NotEnoughPermissions,
      #[cfg(feature = "s3_error")]
      AppError::S3Error(_) => ErrorCode::S3Error,
      AppError::StorageSpaceNotEnough => ErrorCode::StorageSpaceNotEnough,
      AppError::PayloadTooLarge(_) => ErrorCode::PayloadTooLarge,
      AppError::Internal(_) => ErrorCode::Internal,
      AppError::UuidError(_) => ErrorCode::UuidError,
      AppError::IOError(_) => ErrorCode::IOError,
      #[cfg(feature = "sqlx_error")]
      AppError::SqlxError(_) => ErrorCode::SqlxError,
      #[cfg(feature = "validation_error")]
      AppError::ValidatorError(_) => ErrorCode::InvalidRequestParams,
      AppError::S3ResponseError(_) => ErrorCode::S3ResponseError,
      AppError::UrlError(_) => ErrorCode::InvalidUrl,
      AppError::SerdeError(_) => ErrorCode::SerdeError,
      AppError::ReqwestError(_) => ErrorCode::Unhandled,
    }
  }
}

#[cfg(feature = "sqlx_error")]
impl From<sqlx::Error> for AppError {
  fn from(value: sqlx::Error) -> Self {
    let msg = value.to_string();
    match value {
      sqlx::Error::RowNotFound => {
        AppError::RecordNotFound(format!("Record not exist in db. {})", msg))
      },
      _ => AppError::SqlxError(msg),
    }
  }
}

#[cfg(feature = "gotrue_error")]
impl From<crate::gotrue::GoTrueError> for AppError {
  fn from(err: crate::gotrue::GoTrueError) -> Self {
    match (err.code, err.msg.as_str()) {
      (400, m) if m.starts_with("oauth error") => AppError::OAuthError(err.msg),
      (400, m) if m.starts_with("User already registered") => AppError::OAuthError(err.msg),
      (401, _) => AppError::OAuthError(err.msg),
      (422, _) => AppError::InvalidRequestParams(err.msg),
      _ => AppError::Unhandled(format!(
        "gotrue error: {}, message: {}, id: {:?}",
        err.code, err.msg, err.error_id,
      )),
    }
  }
}

#[derive(
  Eq,
  PartialEq,
  Copy,
  Debug,
  Clone,
  serde_repr::Serialize_repr,
  serde_repr::Deserialize_repr,
  Default,
)]
#[repr(i32)]
pub enum ErrorCode {
  #[default]
  Ok = 0,
  Unhandled = -1,
  RecordNotFound = -2,
  RecordAlreadyExists = -3,
  InvalidEmail = 1001,
  InvalidPassword = 1002,
  OAuthError = 1003,
  MissingPayload = 1004,
  DBError = 1005,
  OpenError = 1006,
  InvalidUrl = 1007,
  InvalidRequestParams = 1008,
  UrlMissingParameter = 1009,
  InvalidOAuthProvider = 1010,
  NotLoggedIn = 1011,
  NotEnoughPermissions = 1012,
  #[cfg(feature = "s3_error")]
  S3Error = 1014,
  StorageSpaceNotEnough = 1015,
  PayloadTooLarge = 1016,
  Internal = 1017,
  UuidError = 1018,
  IOError = 1019,
  #[cfg(feature = "sqlx_error")]
  SqlxError = 1020,
  S3ResponseError = 1021,
  SerdeError = 1022,
}

impl ErrorCode {
  pub fn value(&self) -> i32 {
    *self as i32
  }
}

#[derive(Serialize)]
struct AppErrorSerde {
  code: ErrorCode,
  message: String,
}

impl From<&AppError> for AppErrorSerde {
  fn from(value: &AppError) -> Self {
    Self {
      code: value.code(),
      message: value.to_string(),
    }
  }
}

#[cfg(feature = "actix_web_error")]
impl actix_web::error::ResponseError for AppError {
  fn status_code(&self) -> actix_web::http::StatusCode {
    actix_web::http::StatusCode::OK
  }

  fn error_response(&self) -> actix_web::HttpResponse {
    actix_web::HttpResponse::Ok().json(AppErrorSerde::from(self))
  }
}
