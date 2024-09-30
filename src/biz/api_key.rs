use app_error::AppError;
use chrono::NaiveDateTime;
use database_entity::dto::APIKeyPermission;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use sha2::Digest;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_api_key(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
  scopes: Vec<APIKeyPermission>,
  expiration_date: NaiveDateTime,
) -> Result<String, AppError> {
  let is_in_workspace =
    database::workspace::select_user_is_in_workspace(pg_pool, user_uuid, workspace_id).await?;
  if !is_in_workspace {
    return Err(AppError::UserUnAuthorized(format!(
      "user {} is not in workspace {}",
      user_uuid, workspace_id
    )));
  }

  let new_api_key = generate_random_string(32);

  let hashed_api_key = {
    let mut hasher = sha2::Sha256::new();
    hasher.update(new_api_key.as_bytes());
    hasher.finalize().to_vec()
  };

  database::api_key::insert_into_api_key(
    pg_pool,
    user_uuid,
    workspace_id,
    &hashed_api_key,
    scopes,
    expiration_date,
  )
  .await?;

  todo!()
}

fn generate_random_string(length: usize) -> String {
  let random_string: String = thread_rng()
    .sample_iter(&Alphanumeric)
    .take(length)
    .map(char::from)
    .collect();
  random_string
}
