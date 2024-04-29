use std::borrow::Cow;

use axum::{
  http::{status, StatusCode},
  response::{IntoResponse, Redirect},
};

use crate::ext;

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
  Askama(askama::Error),
  GoTrue(gotrue_entity::error::GoTrueError),
  ExtApi(ext::error::Error),
  Redis(redis::RedisError),
  BadRequest(String),
}

impl IntoResponse for WebAppError {
  fn into_response(self) -> axum::response::Response {
    match self {
      WebAppError::Askama(e) => {
        tracing::error!("askama error: {:?}", e);
        status::StatusCode::INTERNAL_SERVER_ERROR.into_response()
      },
      WebAppError::GoTrue(e) => {
        tracing::error!("gotrue error: {:?}", e);
        Redirect::to("/login").into_response()
      },
      WebAppError::ExtApi(e) => e.into_response(),
      WebAppError::Redis(e) => {
        tracing::error!("redis error: {:?}", e);
        status::StatusCode::INTERNAL_SERVER_ERROR.into_response()
      },
      WebAppError::BadRequest(e) => {
        tracing::error!("bad request: {:?}", e);
        status::StatusCode::BAD_REQUEST.into_response()
      },
    }
  }
}

impl From<redis::RedisError> for WebAppError {
  fn from(v: redis::RedisError) -> Self {
    WebAppError::Redis(v)
  }
}

impl From<askama::Error> for WebAppError {
  fn from(v: askama::Error) -> Self {
    WebAppError::Askama(v)
  }
}

impl From<gotrue_entity::error::GoTrueError> for WebAppError {
  fn from(v: gotrue_entity::error::GoTrueError) -> Self {
    WebAppError::GoTrue(v)
  }
}

impl From<ext::error::Error> for WebAppError {
  fn from(v: ext::error::Error) -> Self {
    WebAppError::ExtApi(v)
  }
}

impl From<ext::error::Error> for WebApiError<'_> {
  fn from(v: ext::error::Error) -> Self {
    match v {
      ext::error::Error::NotOk(code, payload) => {
        WebApiError::new(StatusCode::from_u16(code).unwrap(), payload)
      },
      err => WebApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("{:?}", err)),
    }
  }
}
