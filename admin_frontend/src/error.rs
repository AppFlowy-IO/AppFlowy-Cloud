use axum::{http::status, response::IntoResponse};

// #[derive(serde::Serialize)]
pub struct WebApiError {
  pub status_code: status::StatusCode,
  pub payload: String,
}

impl WebApiError {
  pub fn new(code: status::StatusCode, message: String) -> Self {
    Self {
      status_code: code,
      payload: message,
    }
  }
}

impl IntoResponse for WebApiError {
  fn into_response(self) -> axum::response::Response {
    (self.status_code, self.payload).into_response()
  }
}

impl From<gotrue_entity::GoTrueError> for WebApiError {
  fn from(v: gotrue_entity::GoTrueError) -> Self {
    WebApiError::new(status::StatusCode::UNAUTHORIZED, v.to_string())
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
