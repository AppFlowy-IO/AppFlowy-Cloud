use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginRequest {
  pub email: String,
  pub password: String,
}

#[derive(Deserialize)]
pub struct AddUserRequest {
  pub email: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
  pub access_token: String,
}
