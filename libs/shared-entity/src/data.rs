use crate::app_error::AppError;

use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use std::fmt::{Debug, Display};

#[cfg(feature = "cloud")]
pub use crate::data_actix::*;

use crate::error_code::ErrorCode;
/// A macro to generate static AppResponse functions with predefined error codes.
macro_rules! static_app_response {
  ($name:ident, $code:expr) => {
    #[allow(non_snake_case, missing_docs)]
    pub fn $name() -> AppResponse<T> {
      AppResponse::new($code, $code.to_string().into())
    }
  };
}

/// Represents a standardized application response.
///
/// This structure is used to send consistent responses from the server,
/// containing optional data, an error code, and a message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppResponse<T> {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub data: Option<T>,

  #[serde(default)]
  pub code: ErrorCode,

  #[serde(default)]
  pub message: Cow<'static, str>,
}

impl<T> AppResponse<T> {
  pub fn new(code: ErrorCode, message: Cow<'static, str>) -> Self {
    Self {
      data: None,
      code,
      message,
    }
  }

  static_app_response!(Ok, ErrorCode::Ok);
  static_app_response!(Unhandled, ErrorCode::Unhandled);
  static_app_response!(InvalidEmail, ErrorCode::InvalidEmail);
  static_app_response!(InvalidPassword, ErrorCode::InvalidPassword);
  static_app_response!(OAuthError, ErrorCode::OAuthError);

  pub fn split(self) -> (Option<T>, AppError) {
    if self.code == ErrorCode::Ok {
      (self.data, AppError::new(self.code, self.message))
    } else {
      (None, AppError::new(self.code, self.message))
    }
  }

  pub fn into_data(self) -> Result<T, AppError> {
    if self.code == ErrorCode::Ok {
      match self.data {
        None => Err(AppError::new(
          ErrorCode::MissingPayload,
          "missing payload".to_string(),
        )),
        Some(data) => Ok(data),
      }
    } else {
      Err(AppError::new(self.code, self.message))
    }
  }

  pub fn into_error(self) -> Result<(), AppError> {
    if matches!(self.code, ErrorCode::Ok) {
      Ok(())
    } else {
      Err(AppError::new(self.code, self.message))
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
    f.write_fmt(format_args!("{}:{}", self.code, self.message))
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
  T1: Into<AppError>,
{
  fn from(value: T1) -> Self {
    let err: AppError = value.into();
    AppResponse::new(err.code, err.message)
  }
}

impl<T> AppResponse<T>
where
  T: serde::de::DeserializeOwned + 'static,
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
