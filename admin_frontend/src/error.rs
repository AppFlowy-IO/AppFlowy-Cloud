use std::borrow::Cow;

use axum::{http::status, response::IntoResponse};

pub struct WebApiError<'a> {
  pub status_code: status::StatusCode,
  pub payload: Cow<'a, str>,
}

impl<'a> WebApiError<'a> {
  pub fn new<S>(status_code: status::StatusCode, payload: S) -> Self
  where
    S: Into<Cow<'a, str>>,
  {
    WebApiError {
      status_code,
      payload: payload.into(),
    }
  }
}

impl IntoResponse for WebApiError<'_> {
  fn into_response(self) -> axum::response::Response {
    let status = self.status_code;
    let payload = self.payload.into_owned(); // Converts Cow<str> into String
    (status, payload).into_response()
  }
}

impl From<gotrue_entity::GoTrueError> for WebApiError<'_> {
  fn from(v: gotrue_entity::GoTrueError) -> Self {
    WebApiError::new(status::StatusCode::UNAUTHORIZED, v.to_string())
  }
}

impl From<redis::RedisError> for WebApiError<'_> {
  fn from(v: redis::RedisError) -> Self {
    WebApiError::new(status::StatusCode::INTERNAL_SERVER_ERROR, v.to_string())
  }
}

pub struct RenderError(pub askama::Error);

impl From<askama::Error> for RenderError {
  fn from(value: askama::Error) -> Self {
    Self(value)
  }
}

impl IntoResponse for RenderError {
  fn into_response(self) -> axum::response::Response {
    self.0.to_string().into_response()
  }
}
