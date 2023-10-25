use crate::component::auth::jwt::UserUuid;
use anyhow::Context;
use database::collab::upsert_collab_member_with_txn;
use database::user::select_uid_from_email;
use database::workspace::{
  delete_workspace_members, insert_workspace_member_with_txn, select_all_user_workspaces,
  select_workspace, select_workspace_member_list, update_updated_at_of_workspace,
  upsert_workspace_member,
};
use database_entity::dto::{AFAccessLevel, AFRole, AFWorkspace};
use database_entity::pg_row::{AFWorkspaceMemberRow, AFWorkspaceRow};
use shared_entity::app_error::AppError;
use shared_entity::dto::workspace_dto::{CreateWorkspaceMember, WorkspaceMemberChangeset};
use sqlx::{types::uuid, PgPool};
use std::collections::HashMap;
use std::ops::DerefMut;
use uuid::Uuid;

pub async fn get_all_user_workspaces(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
) -> Result<Vec<AFWorkspaceRow>, AppError> {
  let workspaces = select_all_user_workspaces(pg_pool, user_uuid).await?;
  Ok(workspaces)
}

/// Returns the workspace with the given workspace_id and update the updated_at field of the
/// workspace.
pub async fn open_workspace(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
) -> Result<AFWorkspace, AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to open workspace")?;
  let row = select_workspace(txn.deref_mut(), workspace_id).await?;
  update_updated_at_of_workspace(txn.deref_mut(), user_uuid, workspace_id).await?;
  txn
    .commit()
    .await
    .context("Commit transaction to open workspace")?;
  let workspace = AFWorkspace::try_from(row)?;

  Ok(workspace)
}

/// Returns the list of uid of members that are added to the workspace.
/// Adds members to a workspace.
///
/// This function is responsible for adding a list of members to a specified workspace.
/// Each member is associated with a role, which determines their access level within the workspace.
/// The function performs the following operations:
/// 1. Begins a database transaction.
/// 2. For each member:
///    - Determines the access level based on the member's role.
///    - If the member exists (based on their email), inserts them into the workspace and updates their collaboration access level.
/// 3. Commits the database transaction.
///
/// # Returns
/// - A `Result` containing a `HashMap` where the key is the user ID (`uid`) and the value is the role (`AFRole`) assigned to the user in the workspace.
///   If there's an error during the operation, an `AppError` is returned.
///
pub async fn add_workspace_members(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  workspace_id: &Uuid,
  members: Vec<CreateWorkspaceMember>,
) -> Result<HashMap<i64, AFRole>, AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to insert workspace members")?;

  let mut role_by_uid = HashMap::new();
  for member in members.into_iter() {
    let access_level = match &member.role {
      AFRole::Owner => AFAccessLevel::FullAccess,
      AFRole::Member => AFAccessLevel::ReadAndWrite,
      AFRole::Guest => AFAccessLevel::ReadOnly,
    };

    let uid = select_uid_from_email(txn.deref_mut(), &member.email)
      .await
      .map_err(|err| {
        AppError::from(err).with_message(format!(
          "Failed to get uid from email {} when adding workspace members",
          member.email
        ))
      })?;
    insert_workspace_member_with_txn(&mut txn, workspace_id, &member.email, member.role.clone())
      .await?;
    upsert_collab_member_with_txn(uid, workspace_id.to_string(), &access_level, &mut txn).await?;
    role_by_uid.insert(uid, member.role);
  }

  txn
    .commit()
    .await
    .context("Commit transaction to insert workspace members")?;
  Ok(role_by_uid)
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
) -> Result<Vec<AFWorkspaceMemberRow>, AppError> {
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
