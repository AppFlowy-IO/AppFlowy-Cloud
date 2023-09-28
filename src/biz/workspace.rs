use database::workspace::{
  delete_workspace_members, insert_workspace_members, select_all_workspaces_owned,
  select_user_is_workspace_owner, select_workspace_members,
};
use database_entity::{AFRole, AFWorkspaceMember, AFWorkspaces};
use shared_entity::{error::AppError, error_code::ErrorCode};
use sqlx::{types::uuid, PgPool};

pub async fn get_workspaces(
  pg_pool: &PgPool,
  user_uuid: &uuid::Uuid,
) -> Result<AFWorkspaces, AppError> {
  let workspaces = select_all_workspaces_owned(pg_pool, user_uuid).await?;
  Ok(AFWorkspaces(workspaces))
}

pub async fn add_workspace_members(
  pg_pool: &PgPool,
  user_uuid: &uuid::Uuid,
  workspace_uuid: &uuid::Uuid,
  member_emails: &[String],
) -> Result<(), AppError> {
  require_user_is_workspace_owner(pg_pool, user_uuid, workspace_uuid).await?;
  Ok(insert_workspace_members(pg_pool, workspace_uuid, member_emails, AFRole::Member).await?)
}

pub async fn remove_workspace_members(
  pg_pool: &PgPool,
  user_uuid: &uuid::Uuid,
  workspace_uuid: &uuid::Uuid,
  member_emails: &[String],
) -> Result<(), AppError> {
  require_user_is_workspace_owner(pg_pool, user_uuid, workspace_uuid).await?;
  Ok(delete_workspace_members(pg_pool, workspace_uuid, member_emails).await?)
}

pub async fn get_workspace_members(
  pg_pool: &PgPool,
  user_uuid: &uuid::Uuid,
  workspace_uuid: &uuid::Uuid,
) -> Result<Vec<AFWorkspaceMember>, AppError> {
  require_user_is_workspace_owner(pg_pool, user_uuid, workspace_uuid).await?;
  Ok(select_workspace_members(pg_pool, workspace_uuid).await?)
}

async fn require_user_is_workspace_owner(
  pg_pool: &PgPool,
  user_uuid: &uuid::Uuid,
  workspace_uuid: &uuid::Uuid,
) -> Result<(), AppError> {
  match select_user_is_workspace_owner(pg_pool, user_uuid, workspace_uuid).await? {
    true => Ok(()),
    false => Err(ErrorCode::NotEnoughPermissions.into()),
  }
}
