use actix_web::http::StatusCode;
use actix_web::HttpResponse;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
  #[error("Credentials is invalid")]
  InvalidCredentials(#[source] anyhow::Error),

  #[error("User is not exist")]
  UserNotExist(#[source] anyhow::Error),

  #[error("{} is already used", email)]
  UserAlreadyExist { email: String },

  #[error("Invalid password")]
  InvalidPassword,

  #[error("User is unauthorized")]
  Unauthorized,

  #[error("User internal error")]
  InternalError(#[from] anyhow::Error),

  #[error("Parser uuid failed: {}", err)]
  InvalidUuid { err: String },
}

pub fn internal_error(error: anyhow::Error) -> AuthError {
  AuthError::InternalError(error)
}

impl actix_web::error::ResponseError for AuthError {
  fn status_code(&self) -> StatusCode {
    match *self {
      AuthError::InvalidCredentials(_) => StatusCode::UNAUTHORIZED,
      AuthError::UserNotExist(_) => StatusCode::UNAUTHORIZED,
      AuthError::UserAlreadyExist { .. } => StatusCode::BAD_REQUEST,
      AuthError::InvalidPassword => StatusCode::UNAUTHORIZED,
      AuthError::Unauthorized => StatusCode::UNAUTHORIZED,
      AuthError::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
      AuthError::InvalidUuid { .. } => StatusCode::UNAUTHORIZED,
    }
  }

  fn error_response(&self) -> HttpResponse {
    HttpResponse::build(self.status_code()).body(self.to_string())
  }
}

#[derive(Debug, Error)]
pub enum InputParamsError {
  #[error("Invalid name")]
  InvalidName(String),

  #[error("Invalid email format")]
  InvalidEmail(String),

  #[error("Invalid password")]
  InvalidPassword(String),

  #[error("You entered two different new passwords")]
  PasswordNotMatch,
}

impl actix_web::error::ResponseError for InputParamsError {
  fn status_code(&self) -> StatusCode {
    match *self {
      InputParamsError::InvalidName(_) => StatusCode::BAD_REQUEST,
      InputParamsError::InvalidEmail(_) => StatusCode::BAD_REQUEST,
      InputParamsError::InvalidPassword(_) => StatusCode::BAD_REQUEST,
      InputParamsError::PasswordNotMatch => StatusCode::BAD_REQUEST,
    }
  }

  fn error_response(&self) -> HttpResponse {
    HttpResponse::build(self.status_code()).body(self.to_string())
  }
}

#[derive(Debug, Error)]
#[error("Internal server error")]
pub struct InternalServerError {
  error: anyhow::Error,
}

impl InternalServerError {
  pub fn new(error: anyhow::Error) -> Self {
    Self { error }
  }
}

impl actix_web::error::ResponseError for InternalServerError {
  fn status_code(&self) -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
  }

  fn error_response(&self) -> HttpResponse {
    tracing::error!("Internal server error: {}", self.error);
    HttpResponse::build(self.status_code()).body(self.to_string())
  }
}
