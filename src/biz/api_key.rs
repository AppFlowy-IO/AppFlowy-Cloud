use app_error::AppError;
use shared_entity::dto::workspace_dto::APIKeyPermission;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_api_key(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
  scopes: Vec<APIKeyPermission>,
) -> Result<String, AppError> {
  let is_in_workspace =
    database::workspace::select_user_is_in_workspace(pg_pool, user_uuid, workspace_id).await?;
  if !is_in_workspace {
    return Err(AppError::UserUnAuthorized(format!(
      "user {} is not in workspace {}",
      user_uuid, workspace_id
    )));
  }



  todo!()
}
