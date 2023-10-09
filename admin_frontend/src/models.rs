use serde::Deserialize;

#[derive(Deserialize)]
pub struct LoginRequest {
  pub email: String,
  pub password: String,
}
