use serde::Deserialize;

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
  pub confirm_password: String,
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
