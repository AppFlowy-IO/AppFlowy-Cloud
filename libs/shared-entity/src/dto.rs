// Data Transfer Objects (DTO)

use gotrue_entity::{AccessTokenResponse, User};

#[derive(serde::Deserialize, serde::Serialize)]
pub struct WorkspaceMembersParams {
  pub workspace_uuid: uuid::Uuid,
  pub member_emails: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInParams {
  pub email: String,
  pub password: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct UserUpdateParams {
  pub email: String,
  pub password: String,
  pub name: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInPasswordResponse {
  pub access_token_resp: AccessTokenResponse,
  pub is_new: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInTokenResponse {
  pub user: User,
  pub is_new: bool,
}
