use crate::component::auth::jwt::UserUuid;
use anyhow::Context;
use database::workspace::{
  delete_workspace_members, insert_workspace_member, select_all_workspaces_owned,
  select_user_is_workspace_owner, select_workspace_members, upsert_workspace_member,
};
use database_entity::{AFWorkspaceMember, AFWorkspaces};
use shared_entity::dto::workspace_dto::{CreateWorkspaceMember, WorkspaceMemberChangeset};
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
  members: Vec<CreateWorkspaceMember>,
) -> Result<(), AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to insert workspace members")?;
  for member in members {
    insert_workspace_member(&mut txn, workspace_id, member.email, member.role).await?;
  }

  txn
    .commit()
    .await
    .context("Commit transaction to insert workspace members")?;
  Ok(())
}

pub async fn remove_workspace_members(
  user_uuid: &UserUuid,
  pg_pool: &PgPool,
  workspace_id: uuid::Uuid,
  member_emails: Vec<String>,
) -> Result<(), AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to delete workspace members")?;

  for email in member_emails {
    delete_workspace_members(user_uuid, &mut txn, &workspace_id, email).await?;
  }

  txn
    .commit()
    .await
    .context("Commit transaction to delete workspace members")?;
  Ok(())
}

pub async fn get_workspace_members(
  pg_pool: &PgPool,
  _user_uuid: &uuid::Uuid,
  workspace_id: &uuid::Uuid,
) -> Result<Vec<AFWorkspaceMember>, AppError> {
  Ok(select_workspace_members(pg_pool, workspace_id).await?)
}

#[allow(dead_code)]
pub async fn update_workspace_member(
  pg_pool: &PgPool,
  workspace_id: &uuid::Uuid,
  changeset: WorkspaceMemberChangeset,
) -> Result<(), AppError> {
  upsert_workspace_member(pg_pool, workspace_id, &changeset.email, changeset.role).await?;
  Ok(())
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
