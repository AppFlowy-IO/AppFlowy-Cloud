use shared_entity::error::AppError;
use sqlx::PgPool;
use storage_entity::AFWorkspaces;

pub async fn get_workspaces(pg_pool: &PgPool, uuid: &uuid::Uuid) -> Result<AFWorkspaces, AppError> {
  let workspaces = storage::workspace::select_all_workspaces_owned(pg_pool, uuid).await?;
  Ok(AFWorkspaces(workspaces))
}

pub async fn add_workspace_members(
  pg_pool: &PgPool,
  workspace_id: &uuid::Uuid,
  members: Box<[i64]>,
) -> Result<(), AppError> {
  storage::workspace::insert_workspaces_members(pg_pool, workspace_id, &members).await?;
  Ok(())
}
