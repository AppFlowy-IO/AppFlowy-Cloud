use actix_web::web::Json;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use crate::{error::AppError, server_error::ErrorCode};

pub type JsonAppData<T> = Json<AppData<T>>;

#[derive(Debug, Serialize, Deserialize)]
pub struct AppData<T> {
  pub data: Option<T>,
  pub code: ErrorCode,
  pub message: Cow<'static, str>,
}

impl<T> AppData<T> {
  pub fn into_result(self) -> Result<Option<T>, AppError> {
    if self.code == ErrorCode::Ok {
      Ok(self.data)
    } else {
      Err(AppError::new(self.code, self.message.into()))
    }
  }

  #[allow(non_snake_case, missing_docs)]
  pub fn Ok() -> Self {
    Self {
      data: None,
      code: ErrorCode::Ok,
      message: "OK".into(),
    }
  }

  pub fn with_data(mut self, data: T) -> Self {
    self.data = Some(data);
    self
  }

  pub fn with_message(mut self, message: Cow<'static, str>) -> Self {
    self.message = message;
    self
  }
}

impl<T> From<AppData<T>> for JsonAppData<T> {
  fn from(data: AppData<T>) -> Self {
    Json(data)
  }
}
