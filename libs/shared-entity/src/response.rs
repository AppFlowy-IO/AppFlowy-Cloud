use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Cow;

use app_error::AppError;
pub use app_error::ErrorCode;
use serde::de::DeserializeOwned;
use std::fmt::{Debug, Display};

#[cfg(feature = "cloud")]
pub use crate::response_actix::*;

/// A macro to generate static AppResponse functions with predefined error codes.
macro_rules! static_app_response {
  ($name:ident, $error:expr) => {
    #[allow(non_snake_case, missing_docs)]
    pub fn $name() -> AppResponse<T> {
      AppResponse::new($error.code(), $error.to_string())
    }
  }; // ($name:ident, $code:expr, $msg:expr) => {
     //   #[allow(non_snake_case, missing_docs)]
     //   pub fn $name(msg: $msg) -> AppResponse<T> {
     //     AppResponse::new($code, $code(msg.to_string()).to_string())
     //   }
     // };
}

/// Represents a standardized application response.
///
/// This structure is used to send consistent responses from the server,
/// containing optional data, an error code, and a message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppResponse<T> {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub data: Option<T>,

  #[serde(deserialize_with = "default_error_code")]
  pub code: ErrorCode,

  #[serde(default)]
  pub message: Cow<'static, str>,
}

impl<T> AppResponse<T> {
  pub fn new<M: Into<Cow<'static, str>>>(code: ErrorCode, message: M) -> Self {
    Self {
      data: None,
      code,
      message: message.into(),
    }
  }

  static_app_response!(Ok, AppError::Ok);

  pub fn split(self) -> (Option<T>, AppResponseError) {
    if self.is_ok() {
      (self.data, AppResponseError::new(self.code, self.message))
    } else {
      (None, AppResponseError::new(self.code, self.message))
    }
  }

  pub fn into_data(self) -> Result<T, AppResponseError> {
    if self.is_ok() {
      match self.data {
        None => Err(AppResponseError::from(AppError::MissingPayload(
          "".to_string(),
        ))),
        Some(data) => Ok(data),
      }
    } else {
      Err(AppResponseError::new(self.code, self.message))
    }
  }

  pub fn into_error(self) -> Result<(), AppResponseError> {
    if matches!(self.code, ErrorCode::Ok) {
      Ok(())
    } else {
      Err(AppResponseError::new(self.code, self.message))
    }
  }

  pub fn with_data(mut self, data: T) -> Self {
    self.data = Some(data);
    self
  }

  pub fn with_message(mut self, message: impl Into<Cow<'static, str>>) -> Self {
    self.message = message.into();
    self
  }

  pub fn with_code(mut self, code: ErrorCode) -> Self {
    self.code = code;
    self
  }

  pub fn is_ok(&self) -> bool {
    matches!(self.code, ErrorCode::Ok)
  }
}

impl<T> Display for AppResponse<T>
where
  T: Display,
{
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!("{:?}:{}", self.code, self.message))
  }
}

impl<T> std::error::Error for AppResponse<T> where T: Debug + Display {}

/// Provides a conversion from `T1` to `AppResponse<T>`.
///
/// This implementation allows for automatic conversion of any type `T1` that can be
/// transformed into an `AppError` into an `AppResponse<T>`. This is useful for
/// seamlessly converting various error types into a standardized application response.
///
impl<T, T1> From<T1> for AppResponse<T>
where
  T1: Into<AppResponseError>,
{
  fn from(value: T1) -> Self {
    let err: AppResponseError = value.into();
    AppResponse::new(err.code, err.message)
  }
}

impl<T> AppResponse<T>
where
  T: DeserializeOwned + 'static,
{
  pub async fn from_response(resp: reqwest::Response) -> Result<Self, anyhow::Error> {
    let status_code = resp.status();
    if !status_code.is_success() {
      let body = resp.text().await?;
      anyhow::bail!("got error code: {}, body: {}", status_code, body)
    }

    let bytes = resp.bytes().await?;
    let resp = serde_json::from_slice(&bytes)?;
    Ok(resp)
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
pub struct AppResponseError {
  #[serde(deserialize_with = "default_error_code")]
  pub code: ErrorCode,
  pub message: Cow<'static, str>,
}

impl AppResponseError {
  pub fn new(code: ErrorCode, message: impl Into<Cow<'static, str>>) -> Self {
    Self {
      code,
      message: message.into(),
    }
  }

  pub fn is_record_not_found(&self) -> bool {
    matches!(self.code, ErrorCode::RecordNotFound)
  }

  pub fn is_user_unauthorized(&self) -> bool {
    matches!(self.code, ErrorCode::UserUnAuthorized)
  }
}

impl<T> From<T> for AppResponseError
where
  AppError: std::convert::From<T>,
{
  fn from(value: T) -> Self {
    let err = AppError::from(value);
    Self {
      code: err.code(),
      message: Cow::Owned(err.to_string()),
    }
  }
}

impl Display for AppResponseError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!("code:{:?} msg: {}", self.code, self.message))
  }
}

#[cfg(feature = "cloud")]
impl actix_web::error::ResponseError for AppResponseError {
  fn status_code(&self) -> actix_web::http::StatusCode {
    actix_web::http::StatusCode::OK
  }

  fn error_response(&self) -> actix_web::HttpResponse {
    actix_web::HttpResponse::Ok().json(self)
  }
}

/// Uses a custom deserializer for error codes.
/// This ensures that when a client receives an unrecognized error code, due to version mismatches,
/// it defaults to the `Internal` error code.
fn default_error_code<'a, D: Deserializer<'a>>(deserializer: D) -> Result<ErrorCode, D::Error> {
  match ErrorCode::deserialize(deserializer) {
    Ok(code) => Ok(code),
    Err(_) => Ok(ErrorCode::Internal),
  }
}
