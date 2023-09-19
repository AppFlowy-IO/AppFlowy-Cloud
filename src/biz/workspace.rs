use shared_entity::{error::AppError, error_code::ErrorCode};
use sqlx::PgPool;
use storage::workspace::{
  delete_workspace_members, insert_workspace_members, select_all_workspaces_owned,
  select_user_is_workspace_owner,
};
use storage_entity::AFWorkspaces;

pub async fn get_workspaces(pg_pool: &PgPool, uuid: &uuid::Uuid) -> Result<AFWorkspaces, AppError> {
  let workspaces = select_all_workspaces_owned(pg_pool, uuid).await?;
  Ok(AFWorkspaces(workspaces))
}

pub async fn add_workspace_members(
  pg_pool: &PgPool,
  user_uuid: &uuid::Uuid,
  workspace_uuid: &uuid::Uuid,
  member_uids: &[i64],
) -> Result<(), AppError> {
  match select_user_is_workspace_owner(pg_pool, user_uuid, workspace_uuid).await? {
    true => Ok(insert_workspace_members(pg_pool, workspace_uuid, member_uids).await?),
    false => Err(ErrorCode::NotEnoughPermissions.into()),
  }
}

pub async fn remove_workspace_members(
  pg_pool: &PgPool,
  user_uuid: &uuid::Uuid,
  workspace_uuid: &uuid::Uuid,
  member_uids: &[i64],
) -> Result<(), AppError> {
  match select_user_is_workspace_owner(pg_pool, user_uuid, workspace_uuid).await? {
    true => Ok(delete_workspace_members(pg_pool, workspace_uuid, member_uids).await?),
    false => Err(ErrorCode::NotEnoughPermissions.into()),
  }
}
