use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use shared_entity::response::AppResponseError;

#[derive(Deserialize, Debug)]
pub struct AppFlowyCloudError {
  pub code: String,
  pub message: String,
}

#[derive(Debug)]
pub enum Error {
  NotOk(u16, String), // HTTP status code, payload
  Reqwest(reqwest::Error),
  AppFlowyCloud(AppResponseError),
  Unhandled(String),
}

impl From<reqwest::Error> for Error {
  fn from(err: reqwest::Error) -> Self {
    Error::Reqwest(err)
  }
}

impl IntoResponse for Error {
  fn into_response(self) -> Response {
    match self {
      Error::NotOk(status_code, payload) => Response::builder()
        .status(status_code)
        .body(payload.into())
        .unwrap(),
      err => Response::builder()
        .status(500)
        .body(format!("Unhandled error: {:?}", err).into())
        .unwrap(),
    }
  }
}
