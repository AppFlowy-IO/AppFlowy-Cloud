use crate::response::{APIResponse, Code};
use axum::response::IntoResponse;

#[derive(thiserror::Error, Debug)]
pub enum HistoryError {
  #[error(transparent)]
  CollabError(#[from] collab::error::CollabError),

  #[error(transparent)]
  PersistenceError(#[from] sqlx::Error),

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
