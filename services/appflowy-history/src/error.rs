use crate::response::{APIResponse, Code};
use axum::response::IntoResponse;
use tonic::Status;

#[derive(thiserror::Error, Debug)]
pub enum HistoryError {
  #[error(transparent)]
  CollabError(#[from] collab::error::CollabError),

  #[error(transparent)]
  PersistenceError(#[from] sqlx::Error),

  #[error("Try lock fail")]
  TryLockFail,

  #[error("Record not found:{0}")]
  RecordNotFound(String),

  #[error("Apply stale message:{0}")]
  ApplyStaleMessage(String),

  #[error(transparent)]
  RedisStreamError(#[from] collab_stream::error::StreamError),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

impl HistoryError {
  pub fn code(&self) -> Code {
    Code::Unhandled
  }
}

impl IntoResponse for HistoryError {
  fn into_response(self) -> axum::response::Response {
    let code = self.code();
    let message = self.to_string();
    APIResponse::new(())
      .with_message(message)
      .with_code(code)
      .into_response()
  }
}

impl From<HistoryError> for Status {
  fn from(value: HistoryError) -> Self {
    let code = value.code();
    let message = value.to_string();
    Status::new(code.into(), message)
  }
}
