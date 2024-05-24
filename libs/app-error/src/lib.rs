#[cfg(feature = "gotrue_error")]
pub mod gotrue;

#[cfg(feature = "gotrue_error")]
use crate::gotrue::GoTrueError;

#[cfg(feature = "appflowy_ai_error")]
use appflowy_ai_client::error::AIError;
use reqwest::StatusCode;
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

  #[error("{0}")]
  OAuthError(String),

  #[error("{0}")]
  UserUnAuthorized(String),

  #[error("{0}")]
  UserAlreadyRegistered(String),

  #[error("Missing Payload:{0}")]
  MissingPayload(String),

  #[error("Database error:{0}")]
  DBError(String),

  #[error("Open Error:{0}")]
  OpenError(String),

  #[error("Invalid request:{0}")]
  InvalidRequest(String),

  #[error("Invalid OAuth Provider:{0}")]
  InvalidOAuthProvider(String),

  #[error("Not Logged In:{0}")]
  NotLoggedIn(String),

  #[error("{user}: do not have permissions to {action}")]
  NotEnoughPermissions { user: String, action: String },

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

  #[error("{0}")]
  Connect(String),

  #[error("{0}")]
  RequestTimeout(String),

  #[cfg(feature = "tokio_error")]
  #[error(transparent)]
  TokioJoinError(#[from] tokio::task::JoinError),

  #[cfg(feature = "bincode_error")]
  #[error(transparent)]
  BincodeError(#[from] bincode::Error),

  #[error("{0}")]
  NoRequiredData(String),

  #[error("{0}")]
  OverrideWithIncorrectData(String),
}

impl AppError {
  pub fn is_not_enough_permissions(&self) -> bool {
    matches!(self, AppError::NotEnoughPermissions { .. })
  }

  pub fn is_record_not_found(&self) -> bool {
    matches!(self, AppError::RecordNotFound(_))
  }

  pub fn is_network_error(&self) -> bool {
    matches!(self, AppError::Connect(_) | AppError::RequestTimeout(_))
  }

  pub fn is_unauthorized(&self) -> bool {
    matches!(self, AppError::UserUnAuthorized(_))
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
      AppError::UserUnAuthorized(_) => ErrorCode::UserUnAuthorized,
      AppError::UserAlreadyRegistered(_) => ErrorCode::RecordAlreadyExists,
      AppError::MissingPayload(_) => ErrorCode::MissingPayload,
      AppError::DBError(_) => ErrorCode::DBError,
      AppError::OpenError(_) => ErrorCode::OpenError,
      AppError::InvalidOAuthProvider(_) => ErrorCode::InvalidOAuthProvider,
      AppError::InvalidRequest(_) => ErrorCode::InvalidRequest,
      AppError::NotLoggedIn(_) => ErrorCode::NotLoggedIn,
      AppError::NotEnoughPermissions { .. } => ErrorCode::NotEnoughPermissions,
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
      AppError::ValidatorError(_) => ErrorCode::InvalidRequest,
      AppError::S3ResponseError(_) => ErrorCode::S3ResponseError,
      AppError::UrlError(_) => ErrorCode::InvalidUrl,
      AppError::SerdeError(_) => ErrorCode::SerdeError,
      AppError::Connect(_) => ErrorCode::NetworkError,
      AppError::RequestTimeout(_) => ErrorCode::NetworkError,
      #[cfg(feature = "tokio_error")]
      AppError::TokioJoinError(_) => ErrorCode::Internal,
      #[cfg(feature = "bincode_error")]
      AppError::BincodeError(_) => ErrorCode::Internal,
      AppError::NoRequiredData(_) => ErrorCode::NoRequiredData,
      AppError::OverrideWithIncorrectData(_) => ErrorCode::OverrideWithIncorrectData,
    }
  }
}

impl From<reqwest::Error> for AppError {
  fn from(error: reqwest::Error) -> Self {
    #[cfg(not(target_arch = "wasm32"))]
    if error.is_connect() {
      return AppError::Connect(error.to_string());
    }

    if error.is_timeout() {
      return AppError::RequestTimeout(error.to_string());
    }

    if error.is_request() {
      return if error.status() == Some(StatusCode::PAYLOAD_TOO_LARGE) {
        AppError::PayloadTooLarge(error.to_string())
      } else {
        AppError::InvalidRequest(error.to_string())
      };
    }
    AppError::Unhandled(error.to_string())
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
    match err {
      GoTrueError::Connect(msg) => AppError::Connect(msg),
      GoTrueError::RequestTimeout(msg) => AppError::RequestTimeout(msg),
      GoTrueError::InvalidRequest(msg) => AppError::InvalidRequest(msg),
      GoTrueError::ClientError(err) => AppError::OAuthError(err.to_string()),
      GoTrueError::Auth(err) => AppError::UserUnAuthorized(err),
      GoTrueError::Internal(err) => match (err.code, err.msg.as_str()) {
        (400, m) if m.starts_with("oauth error") => AppError::OAuthError(err.msg),
        (400, m) if m.starts_with("User already registered") => {
          AppError::UserAlreadyRegistered(err.msg)
        },
        (401, _) => AppError::UserUnAuthorized(format!("{}:{}", err.code, err.msg)),
        (422, _) => AppError::InvalidRequest(err.msg),
        _ => AppError::OAuthError(err.msg),
      },
      GoTrueError::Unhandled(err) => AppError::Internal(err),
      GoTrueError::NotLoggedIn(msg) => AppError::NotLoggedIn(msg),
    }
  }
}

#[cfg_attr(target_arch = "wasm32", derive(tsify::Tsify))]
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
  InvalidRequest = 1008,
  InvalidOAuthProvider = 1009,
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
  NetworkError = 1023,
  UserUnAuthorized = 1024,
  NoRequiredData = 1025,
  WorkspaceLimitExceeded = 1026,
  WorkspaceMemberLimitExceeded = 1027,
  FileStorageLimitExceeded = 1028,
  OverrideWithIncorrectData = 1029,
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

#[cfg(feature = "appflowy_ai_error")]
impl From<AIError> for AppError {
  fn from(err: AIError) -> Self {
    match err {
      AIError::Internal(err) => AppError::Internal(err),
      AIError::RequestTimeout(err) => AppError::RequestTimeout(err),
      AIError::PayloadTooLarge(err) => AppError::PayloadTooLarge(err),
      AIError::InvalidRequest(err) => AppError::InvalidRequest(err),
    }
  }
}
