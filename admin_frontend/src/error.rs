use axum::{response::IntoResponse, Json};

#[derive(serde::Serialize)]
pub struct WebApiError {
  pub code: i16,
  pub message: String,
}

impl WebApiError {
  pub fn new(code: i16, message: String) -> Self {
    Self { code, message }
  }
}

impl IntoResponse for WebApiError {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

impl From<gotrue_entity::GoTrueError> for WebApiError {
  fn from(v: gotrue_entity::GoTrueError) -> Self {
    WebApiError::new(500, v.to_string())
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
