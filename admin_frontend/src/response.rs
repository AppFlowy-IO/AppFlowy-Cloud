use std::borrow::Cow;

use axum::{response::IntoResponse, Json};

#[derive(serde::Serialize)]
pub struct WebApiResponse<T>
where
  T: serde::Serialize,
{
  pub code: i16,
  pub message: Cow<'static, str>,
  pub data: T,
}

impl<T> WebApiResponse<T>
where
  T: serde::Serialize,
{
  pub fn new(message: Cow<'static, str>, data: T) -> Self {
    Self {
      code: 0,
      message,
      data,
    }
  }
}

impl<T> IntoResponse for WebApiResponse<T>
where
  T: serde::Serialize,
{
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

impl<T> From<T> for WebApiResponse<T>
where
  T: serde::Serialize,
{
  fn from(data: T) -> Self {
    Self::new("success".into(), data)
  }
}

impl WebApiResponse<()> {
  pub fn from_str(message: Cow<'static, str>) -> Self {
    Self::new(message, ())
  }
}
