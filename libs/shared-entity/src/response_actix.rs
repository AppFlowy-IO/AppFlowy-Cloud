use serde::Serialize;

use crate::response::AppResponse;
use actix_web::web::Json;
use std::fmt::{Debug, Display};

pub type JsonAppResponse<T> = Json<AppResponse<T>>;

impl<T> From<AppResponse<T>> for JsonAppResponse<T> {
  fn from(data: AppResponse<T>) -> Self {
    actix_web::web::Json(data)
  }
}

impl<T> actix_web::error::ResponseError for AppResponse<T>
where
  T: Debug + Display + Clone + Serialize,
{
  fn status_code(&self) -> actix_web::http::StatusCode {
    actix_web::http::StatusCode::OK
  }

  fn error_response(&self) -> actix_web::HttpResponse {
    actix_web::HttpResponse::Ok().json(self)
  }
}
