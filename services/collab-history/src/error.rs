use crate::response::{APIResponse, Code};
use axum::response::IntoResponse;

#[derive(thiserror::Error, Debug)]
pub enum Error {
  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

impl Error {
  pub fn code(&self) -> Code {
    match self {
      Error::Internal(_) => Code::Unhandled,
    }
  }
}

impl IntoResponse for Error {
  fn into_response(self) -> axum::response::Response {
    let code = self.code();
    let message = self.to_string();
    APIResponse::new(())
      .with_message(message)
      .with_code(code)
      .into_response()
  }
}
