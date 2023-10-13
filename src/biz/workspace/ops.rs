use database::workspace::{
  delete_workspace_members, insert_workspace_members, select_all_workspaces_owned,
  select_user_is_workspace_owner, select_workspace_members,
};
use database_entity::{AFRole, AFWorkspaceMember, AFWorkspaces};
use shared_entity::{app_error::AppError, error_code::ErrorCode};
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
  _user_uuid: &uuid::Uuid,
  workspace_id: &uuid::Uuid,
  member_emails: &[String],
) -> Result<(), AppError> {
  Ok(insert_workspace_members(pg_pool, workspace_id, member_emails, AFRole::Member).await?)
}

pub async fn remove_workspace_members(
  pg_pool: &PgPool,
  _user_uuid: &uuid::Uuid,
  workspace_id: &uuid::Uuid,
  member_emails: &[String],
) -> Result<(), AppError> {
  Ok(delete_workspace_members(pg_pool, workspace_id, member_emails).await?)
}

pub async fn get_workspace_members(
  pg_pool: &PgPool,
  _user_uuid: &uuid::Uuid,
  workspace_id: &uuid::Uuid,
) -> Result<Vec<AFWorkspaceMember>, AppError> {
  Ok(select_workspace_members(pg_pool, workspace_id).await?)
}

#[allow(dead_code)]
pub async fn update_workspace_member_permission(
  _pg_pool: &PgPool,
  _user_uuid: &uuid::Uuid,
  _workspace_id: &uuid::Uuid,
  _member_emails: &[String],
) -> Result<(), AppError> {
  todo!()
}

pub async fn require_user_is_workspace_owner(
  pg_pool: &PgPool,
  user_uuid: &uuid::Uuid,
  workspace_uuid: &uuid::Uuid,
) -> Result<(), AppError> {
  match select_user_is_workspace_owner(pg_pool, user_uuid, workspace_uuid).await? {
    true => Ok(()),
    false => Err(ErrorCode::NotEnoughPermissions.into()),
  }
}
