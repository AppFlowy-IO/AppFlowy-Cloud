#[cfg(feature = "gotrue_error")]
pub mod gotrue;

#[cfg(feature = "gotrue_error")]
use crate::gotrue::GoTrueError;
#[cfg(feature = "appflowy_ai_error")]
use appflowy_ai_client::error::AIError;
use reqwest::StatusCode;
use serde::Serialize;
use std::error::Error;
use std::string::FromUtf8Error;
use thiserror::Error;
use uuid::Uuid;

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

  #[error("Invalid page data:{0}")]
  InvalidPageData(String),

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

  #[error("User does not have permissions to execute this action")]
  NotEnoughPermissions,

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

  #[cfg(feature = "sqlx_error")]
  #[error("{desc}: {err}")]
  SqlxArgEncodingError {
    desc: String,
    err: Box<dyn std::error::Error + 'static + Send + Sync>,
  },

  #[cfg(feature = "validation_error")]
  #[error(transparent)]
  ValidatorError(#[from] validator::ValidationErrors),

  #[error(transparent)]
  UrlError(#[from] url::ParseError),

  #[error(transparent)]
  SerdeError(#[from] serde_json::Error),

  #[error(transparent)]
  Utf8Error(#[from] FromUtf8Error),

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

  #[error("{0}")]
  PublishNamespaceAlreadyTaken(String),

  #[error("{0}")]
  AIServiceUnavailable(String),

  #[error("{0}")]
  StringLengthLimitReached(String),

  #[error("{0}")]
  InvalidContentType(String),

  #[error("{0}")]
  InvalidPublishedOutline(String),

  #[error("{0}")]
  InvalidFolderView(String),

  #[error("{0}")]
  NotInviteeOfWorkspaceInvitation(String),

  #[error("{0}")]
  MissingView(String),

  #[error("{0}")]
  TooManyImportTask(String),

  #[error("There is existing access request for workspace {workspace_id} and view {view_id}")]
  AccessRequestAlreadyExists { workspace_id: Uuid, view_id: Uuid },

  #[error("There is existing published view for workspace {workspace_id} with publish_name {publish_name}")]
  PublishNameAlreadyExists {
    workspace_id: Uuid,
    publish_name: String,
  },

  #[error("There is an invalid character in the publish name: {character}")]
  PublishNameInvalidCharacter { character: char },

  #[error("The publish name is too long, given length: {given_length}, max length: {max_length}")]
  PublishNameTooLong {
    given_length: usize,
    max_length: usize,
  },

  #[error("There is an invalid character in the publish namespace: {character}")]
  CustomNamespaceInvalidCharacter { character: char },

  #[error("{0}")]
  ServiceTemporaryUnavailable(String),

  #[error("Decode update error: {0}")]
  DecodeUpdateError(String),

  #[error("{0}")]
  ActionTimeout(String),

  #[error("Apply update error:{0}")]
  ApplyUpdateError(String),

  #[error("{0}")]
  InvalidBlock(String),

  #[error("{0}")]
  FeatureNotAvailable(String),

  #[error("unable to find invitation code")]
  InvalidInvitationCode,

  #[error("{0} is already a member of the workspace")]
  InvalidGuest(String),

  #[error("free plan workspace guest limit exceeded")]
  FreePlanGuestLimitExceeded,

  #[error("paid plan workspace guest limit exceeded")]
  PaidPlanGuestLimitExceeded,
}

impl AppError {
  pub fn is_not_enough_permissions(&self) -> bool {
    matches!(self, AppError::NotEnoughPermissions)
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
      AppError::NotEnoughPermissions => ErrorCode::NotEnoughPermissions,
      AppError::StorageSpaceNotEnough => ErrorCode::StorageSpaceNotEnough,
      AppError::PayloadTooLarge(_) => ErrorCode::PayloadTooLarge,
      AppError::Internal(_) => ErrorCode::Internal,
      AppError::UuidError(_) => ErrorCode::UuidError,
      AppError::IOError(_) => ErrorCode::IOError,
      #[cfg(feature = "sqlx_error")]
      AppError::SqlxError(_) => ErrorCode::SqlxError,
      #[cfg(feature = "sqlx_error")]
      AppError::SqlxArgEncodingError { .. } => ErrorCode::SqlxArgEncodingError,
      #[cfg(feature = "validation_error")]
      AppError::ValidatorError(_) => ErrorCode::InvalidRequest,
      AppError::S3ResponseError(_) => ErrorCode::S3ResponseError,
      AppError::UrlError(_) => ErrorCode::InvalidUrl,
      AppError::SerdeError(_) => ErrorCode::SerdeError,
      AppError::Connect(_) => ErrorCode::NetworkError,
      AppError::RequestTimeout(_) => ErrorCode::RequestTimeout,
      #[cfg(feature = "tokio_error")]
      AppError::TokioJoinError(_) => ErrorCode::Internal,
      #[cfg(feature = "bincode_error")]
      AppError::BincodeError(_) => ErrorCode::Internal,
      AppError::NoRequiredData(_) => ErrorCode::NoRequiredData,
      AppError::OverrideWithIncorrectData(_) => ErrorCode::OverrideWithIncorrectData,
      AppError::Utf8Error(_) => ErrorCode::Internal,
      AppError::PublishNamespaceAlreadyTaken(_) => ErrorCode::PublishNamespaceAlreadyTaken,
      AppError::AIServiceUnavailable(_) => ErrorCode::AIServiceUnavailable,
      AppError::StringLengthLimitReached(_) => ErrorCode::StringLengthLimitReached,
      AppError::InvalidContentType(_) => ErrorCode::InvalidContentType,
      AppError::InvalidPublishedOutline(_) => ErrorCode::InvalidPublishedOutline,
      AppError::InvalidFolderView(_) => ErrorCode::InvalidFolderView,
      AppError::InvalidPageData(_) => ErrorCode::InvalidPageData,
      AppError::NotInviteeOfWorkspaceInvitation(_) => ErrorCode::NotInviteeOfWorkspaceInvitation,
      AppError::MissingView(_) => ErrorCode::MissingView,
      AppError::AccessRequestAlreadyExists { .. } => ErrorCode::AccessRequestAlreadyExists,
      AppError::TooManyImportTask(_) => ErrorCode::TooManyImportTask,
      AppError::PublishNameAlreadyExists { .. } => ErrorCode::PublishNameAlreadyExists,
      AppError::PublishNameInvalidCharacter { .. } => ErrorCode::PublishNameInvalidCharacter,
      AppError::PublishNameTooLong { .. } => ErrorCode::PublishNameTooLong,
      AppError::CustomNamespaceInvalidCharacter { .. } => {
        ErrorCode::CustomNamespaceInvalidCharacter
      },
      AppError::ServiceTemporaryUnavailable(_) => ErrorCode::ServiceTemporaryUnavailable,
      AppError::DecodeUpdateError(_) => ErrorCode::DecodeUpdateError,
      AppError::ApplyUpdateError(_) => ErrorCode::ApplyUpdateError,
      AppError::ActionTimeout(_) => ErrorCode::ActionTimeout,
      AppError::InvalidBlock(_) => ErrorCode::InvalidBlock,
      AppError::FeatureNotAvailable(_) => ErrorCode::FeatureNotAvailable,
      AppError::InvalidInvitationCode => ErrorCode::InvalidInvitationCode,
      AppError::InvalidGuest(_) => ErrorCode::InvalidGuest,
      AppError::FreePlanGuestLimitExceeded => ErrorCode::FreePlanGuestLimitExceeded,
      AppError::PaidPlanGuestLimitExceeded => ErrorCode::PaidPlanGuestLimitExceeded,
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
      return AppError::ServiceTemporaryUnavailable(error.to_string());
    }

    if let Some(cause) = error.source() {
      if cause
        .to_string()
        .contains("connection closed before message completed")
      {
        return AppError::ServiceTemporaryUnavailable(error.to_string());
      }
    }

    // Handle request-related errors
    if let Some(status_code) = error.status() {
      if error.is_request() {
        match status_code {
          StatusCode::PAYLOAD_TOO_LARGE => {
            return AppError::PayloadTooLarge(error.to_string());
          },
          status_code if status_code.is_server_error() => {
            return AppError::ServiceTemporaryUnavailable(error.to_string());
          },
          _ => {
            return AppError::InvalidRequest(error.to_string());
          },
        }
      }
    }

    AppError::Internal(error.into())
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
      sqlx::Error::PoolTimedOut => AppError::ActionTimeout(value.to_string()),
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

impl From<String> for AppError {
  fn from(err: String) -> Self {
    AppError::Unhandled(err)
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
  PublishNamespaceNotSet = 1030,
  PublishNamespaceAlreadyTaken = 1031,
  AIServiceUnavailable = 1032,
  AIResponseLimitExceeded = 1033,
  StringLengthLimitReached = 1034,
  #[cfg(feature = "sqlx_error")]
  SqlxArgEncodingError = 1035,
  InvalidContentType = 1036,
  SingleUploadLimitExceeded = 1037,
  AppleRevokeTokenError = 1038,
  InvalidPublishedOutline = 1039,
  InvalidFolderView = 1040,
  NotInviteeOfWorkspaceInvitation = 1041,
  MissingView = 1042,
  AccessRequestAlreadyExists = 1043,
  CustomNamespaceDisabled = 1044,
  CustomNamespaceDisallowed = 1045,
  TooManyImportTask = 1046,
  CustomNamespaceTooShort = 1047,
  CustomNamespaceTooLong = 1048,
  CustomNamespaceReserved = 1049,
  PublishNameAlreadyExists = 1050,
  PublishNameInvalidCharacter = 1051,
  PublishNameTooLong = 1052,
  CustomNamespaceInvalidCharacter = 1053,
  ServiceTemporaryUnavailable = 1054,
  DecodeUpdateError = 1055,
  ApplyUpdateError = 1056,
  ActionTimeout = 1057,
  AIImageResponseLimitExceeded = 1058,
  MailerError = 1059,
  LicenseError = 1060,
  AIMaxRequired = 1061,
  InvalidPageData = 1062,
  MemberNotFound = 1063,
  InvalidBlock = 1064,
  RequestTimeout = 1065,
  AIResponseError = 1066,
  FeatureNotAvailable = 1067,
  InvalidInvitationCode = 1068,
  InvalidGuest = 1069,
  FreePlanGuestLimitExceeded = 1070,
  PaidPlanGuestLimitExceeded = 1071,
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
      AIError::SerdeError(err) => AppError::SerdeError(err),
      AIError::ServiceUnavailable(err) => AppError::AIServiceUnavailable(err),
    }
  }
}

#[cfg(feature = "appflowy_ai_error")]
impl From<async_openai::error::OpenAIError> for AppError {
  fn from(err: async_openai::error::OpenAIError) -> Self {
    match &err {
      async_openai::error::OpenAIError::Reqwest(e) => AppError::InvalidRequest(e.to_string()),
      async_openai::error::OpenAIError::ApiError(e) => AppError::InvalidRequest(e.to_string()),
      async_openai::error::OpenAIError::InvalidArgument(e) => {
        AppError::InvalidRequest(e.to_string())
      },
      _ => AppError::Internal(err.into()),
    }
  }
}

use tokio_tungstenite::tungstenite::Error as TungsteniteError;

impl From<TungsteniteError> for AppError {
  fn from(err: TungsteniteError) -> Self {
    match &err {
      TungsteniteError::Http(resp) => {
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED.as_u16() || status == StatusCode::NOT_FOUND.as_u16() {
          AppError::UserUnAuthorized("Unauthorized websocket connection".to_string())
        } else {
          AppError::Internal(err.into())
        }
      },
      _ => AppError::Internal(err.into()),
    }
  }
}

impl From<tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue> for AppError {
  fn from(err: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue) -> Self {
    AppError::InvalidRequest(err.to_string())
  }
}
