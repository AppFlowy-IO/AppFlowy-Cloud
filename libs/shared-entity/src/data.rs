use serde::{Deserialize, Serialize};

use crate::{error::AppError, server_error::ErrorCode};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppData<T> {
  pub data: Option<T>,
  pub code: ErrorCode,
  pub message: String,
}

impl<T> AppData<T> {
  pub fn into_result(self) -> Result<Option<T>, AppError> {
    if self.code == ErrorCode::Ok {
      Ok(self.data)
    } else {
      Err(AppError::new(self.code, self.message))
    }
  }
}

pub fn app_ok() -> AppData<()> {
  AppData {
    data: None,
    code: ErrorCode::Ok,
    message: "OK".to_string(),
  }
}

pub fn app_ok_data<T>(data: T) -> AppData<T> {
  AppData {
    data: Some(data),
    code: ErrorCode::Ok,
    message: "OK".to_string(),
  }
}
