use app_error::AppError;
use chrono::NaiveDateTime;
use database_entity::dto::APIKeyPermission;
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

  let api_key_hash = "hash".to_string(); // TODO

  database::api_key::insert_into_api_key(
    pg_pool,
    user_uuid,
    workspace_id,
    api_key_hash,
    scopes,
    expiration_date,
  )
  .await?;

  todo!()
}
