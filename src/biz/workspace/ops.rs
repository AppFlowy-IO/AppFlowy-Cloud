use crate::component::auth::jwt::UserUuid;
use anyhow::Context;
use database::collab::upsert_collab_member_with_txn;
use database::user::select_uid_from_email;
use database::workspace::{
  delete_workspace_members, insert_workspace_member_with_txn, select_all_workspaces_owned,
  select_workspace_member_list, upsert_workspace_member,
};
use database_entity::{AFAccessLevel, AFRole, AFWorkspaceMember, AFWorkspaces};
use shared_entity::app_error::AppError;
use shared_entity::dto::workspace_dto::{CreateWorkspaceMember, WorkspaceMemberChangeset};
use sqlx::{types::uuid, PgPool};
use std::ops::DerefMut;
use uuid::Uuid;

pub async fn get_workspaces(pg_pool: &PgPool, user_uuid: &Uuid) -> Result<AFWorkspaces, AppError> {
  let workspaces = select_all_workspaces_owned(pg_pool, user_uuid).await?;
  Ok(AFWorkspaces(workspaces))
}

pub async fn add_workspace_members(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  workspace_id: &Uuid,
  members: &[CreateWorkspaceMember],
) -> Result<(), AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to insert workspace members")?;

  for member in members {
    let access_level = match &member.role {
      AFRole::Owner => AFAccessLevel::FullAccess,
      AFRole::Member => AFAccessLevel::ReadAndWrite,
      AFRole::Guest => AFAccessLevel::ReadOnly,
    };
    let uid = select_uid_from_email(txn.deref_mut(), &member.email).await?;

    insert_workspace_member_with_txn(&mut txn, workspace_id, &member.email, member.role.clone())
      .await?;
    upsert_collab_member_with_txn(uid, workspace_id.to_string(), &access_level, &mut txn).await?;
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
  workspace_id: &Uuid,
  member_emails: &[String],
) -> Result<(), AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to delete workspace members")?;

  for email in member_emails {
    delete_workspace_members(user_uuid, &mut txn, workspace_id, email.as_str()).await?;
  }

  txn
    .commit()
    .await
    .context("Commit transaction to delete workspace members")?;
  Ok(())
}

pub async fn get_workspace_members(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  workspace_id: &Uuid,
) -> Result<Vec<AFWorkspaceMember>, AppError> {
  Ok(select_workspace_member_list(pg_pool, workspace_id).await?)
}

pub async fn update_workspace_member(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  changeset: &WorkspaceMemberChangeset,
) -> Result<(), AppError> {
  upsert_workspace_member(
    pg_pool,
    workspace_id,
    &changeset.email,
    changeset.role.clone(),
  )
  .await?;
  Ok(())
}
