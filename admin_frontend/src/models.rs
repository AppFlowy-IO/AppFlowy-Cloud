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
  pub email: String,
  pub password: String,
}
