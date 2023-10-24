use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct WebApiLoginRequest {
  pub email: String,
  pub password: String,
}

#[derive(Deserialize)]
pub struct WebApiPutUserRequest {
  pub password: String,
}

#[derive(Deserialize)]
pub struct WebApiChangePasswordRequest {
  pub new_password: String,
}

#[derive(Deserialize)]
pub struct WebApiAdminCreateUserRequest {
  pub email: String,
  pub password: String,
  pub require_email_verification: bool,
}

#[derive(Deserialize)]
pub struct WebApiInviteUserRequest {
  pub email: String,
}

#[derive(Serialize)]
pub struct WebApiOpenAppResponse {
  pub link: String,
}
