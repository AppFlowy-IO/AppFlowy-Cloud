use axum::{response::IntoResponse, Json};

#[derive(serde::Serialize)]
pub struct WebApiResponse<T>
where
  T: serde::Serialize,
{
  pub code: i16,
  pub message: String,
  pub data: T,
}

impl<T> WebApiResponse<T>
where
  T: serde::Serialize,
{
  pub fn new(data: T) -> Self {
    Self {
      code: 0,
      message: "success".to_owned(),
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
