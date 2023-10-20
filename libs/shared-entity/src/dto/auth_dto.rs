// Data Transfer Objects (DTO)

use gotrue_entity::AccessTokenResponse;

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInParams {
  pub email: String,
  pub password: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct UpdateUsernameParams {
  pub new_name: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInPasswordResponse {
  pub access_token_resp: AccessTokenResponse,
  pub is_new: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInTokenResponse {
  pub is_new: bool,
}
