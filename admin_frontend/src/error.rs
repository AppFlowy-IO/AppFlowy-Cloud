use std::borrow::Cow;

use axum::{
  http::status,
  response::{IntoResponse, Redirect},
};

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

impl From<gotrue_entity::error::GoTrueError> for WebApiError<'_> {
  fn from(v: gotrue_entity::error::GoTrueError) -> Self {
    WebApiError::new(status::StatusCode::UNAUTHORIZED, v.to_string())
  }
}

impl From<redis::RedisError> for WebApiError<'_> {
  fn from(v: redis::RedisError) -> Self {
    WebApiError::new(status::StatusCode::INTERNAL_SERVER_ERROR, v.to_string())
  }
}

pub enum WebAppError {
  AskamaError(askama::Error),
  GoTrueError(gotrue_entity::error::GoTrueError),
}

impl IntoResponse for WebAppError {
  fn into_response(self) -> axum::response::Response {
    match self {
      WebAppError::AskamaError(e) => {
        tracing::error!("askama error: {:?}", e);
        status::StatusCode::INTERNAL_SERVER_ERROR.into_response()
      },
      WebAppError::GoTrueError(e) => {
        tracing::error!("gotrue error: {:?}", e);
        Redirect::to("/login").into_response()
      },
    }
  }
}

impl From<askama::Error> for WebAppError {
  fn from(v: askama::Error) -> Self {
    WebAppError::AskamaError(v)
  }
}

impl From<gotrue_entity::error::GoTrueError> for WebAppError {
  fn from(v: gotrue_entity::error::GoTrueError) -> Self {
    WebAppError::GoTrueError(v)
  }
}
