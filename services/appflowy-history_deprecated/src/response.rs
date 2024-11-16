use axum::response::IntoResponse;
use axum::Json;

#[derive(serde::Serialize)]
pub struct APIResponse<T> {
  pub code: Code,
  pub message: String,
  pub data: T,
}

impl<T> APIResponse<T> {
  pub fn new(data: T) -> Self {
    Self {
      code: Code::Ok,
      message: "success".to_string(),
      data,
    }
  }

  pub fn with_message(self, message: String) -> Self {
    Self { message, ..self }
  }

  pub fn with_code(self, code: Code) -> Self {
    Self { code, ..self }
  }
}

impl<T> IntoResponse for APIResponse<T>
where
  T: serde::Serialize,
{
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

impl<T> From<T> for APIResponse<T>
where
  T: serde::Serialize,
{
  fn from(data: T) -> Self {
    Self::new(data)
  }
}

#[derive(Debug, Clone, serde_repr::Serialize_repr, serde_repr::Deserialize_repr, Default)]
#[repr(i32)]
pub enum Code {
  #[default]
  Ok = 0,
  Unhandled = -1,
}

impl From<Code> for tonic::Code {
  fn from(code: Code) -> Self {
    match code {
      Code::Ok => tonic::Code::Ok,
      Code::Unhandled => tonic::Code::Unknown,
    }
  }
}
