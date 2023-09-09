use actix_web::http::StatusCode;
use actix_web::web::Json;
use actix_web::HttpResponse;
use gotrue::models::{GoTrueError, OAuthError};

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::{Debug, Display};

use crate::server_error::ErrorCode;
macro_rules! static_app_response {
  ($name:ident, $code:expr) => {
    #[allow(non_snake_case, missing_docs)]
    pub fn $name() -> AppResponse<T> {
      AppResponse::new($code, $code.to_string().into())
    }
  };
}
pub type JsonAppResponse<T> = Json<AppResponse<T>>;

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

  pub fn into_inner(self) -> Result<Option<T>, anyhow::Error> {
    if self.code == ErrorCode::Ok {
      Ok(self.data)
    } else {
      Err(self.code.into())
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
    self.code == ErrorCode::Ok
  }

  pub fn unwrap_error(self) -> AppError {
    AppError::new(self.code, self.message)
  }
}

impl<T> Display for AppResponse<T>
where
  T: Display,
{
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!("{}:{}]", self.code, self.message,))
  }
}

impl<T> From<AppResponse<T>> for JsonAppResponse<T> {
  fn from(data: AppResponse<T>) -> Self {
    Json(data)
  }
}

impl<T> std::error::Error for AppResponse<T> where T: Debug + Display {}

impl<T> From<anyhow::Error> for AppResponse<T>
where
  T: Debug + Display + Send + Sync + 'static,
{
  fn from(err: anyhow::Error) -> Self {
    err
      .downcast::<AppResponse<T>>()
      .unwrap_or_else(|err| AppResponse::Unhandled().with_message(err.to_string()))
  }
}

impl<T> From<GoTrueError> for AppResponse<T> {
  fn from(err: GoTrueError) -> Self {
    AppResponse::Unhandled().with_message(format!(
      "gotrue error: {}, id: {}",
      err.code,
      err.error_id.unwrap_or("".to_string())
    ))
  }
}

impl<T> From<OAuthError> for AppResponse<T> {
  fn from(err: OAuthError) -> Self {
    AppResponse::OAuthError().with_message(err.to_string())
  }
}

impl From<AppError> for AppResponse<()> {
  fn from(err: AppError) -> Self {
    AppResponse::new(err.code, err.message)
  }
}

impl<T> actix_web::error::ResponseError for AppResponse<T>
where
  T: Debug + Display + Clone + Serialize,
{
  fn status_code(&self) -> StatusCode {
    StatusCode::OK
  }

  fn error_response(&self) -> HttpResponse {
    HttpResponse::Ok().json(self)
  }
}
