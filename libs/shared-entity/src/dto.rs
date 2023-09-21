// Data Transfer Objects (DTO)

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
