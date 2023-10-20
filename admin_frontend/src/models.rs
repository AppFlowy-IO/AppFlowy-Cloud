use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginRequest {
  pub email: String,
  pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
  pub access_token: String,
}

#[derive(Deserialize)]
pub struct PutUserRequest {
  pub password: String,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
  pub new_password: String,
}

#[derive(Deserialize)]
pub struct WebAdminCreateUserRequest {
  pub email: String,
  pub password: String,
  pub require_email_verification: bool,
}
