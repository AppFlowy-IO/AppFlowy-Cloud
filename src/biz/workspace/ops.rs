use crate::biz::workspace::access_control::WorkspaceAccessControl;
use crate::mailer::Mailer;
use crate::state::GoTrueAdmin;
use anyhow::Context;
use app_error::AppError;
use database::collab::upsert_collab_member_with_txn;
use database::file::bucket_s3_impl::BucketClientS3Impl;
use database::file::BucketStorage;
use database::pg_row::{AFWorkspaceMemberRow, AFWorkspaceRow};
use database::resource_usage::get_all_workspace_blob_metadata;
use database::user::select_uid_from_email;
use database::workspace::{
  change_workspace_icon, delete_from_workspace, delete_workspace_members, get_invitation_by_id,
  insert_user_workspace, insert_workspace_invitation, rename_workspace, select_all_user_workspaces,
  select_user_is_workspace_owner, select_workspace, select_workspace_invitations_for_user,
  select_workspace_member_list, select_workspace_total_collab_bytes,
  update_updated_at_of_workspace, update_workspace_invitation_set_status_accepted,
  upsert_workspace_member, upsert_workspace_member_with_txn,
};
use database_entity::dto::{
  AFAccessLevel, AFRole, AFWorkspace, AFWorkspaceInvitation, AFWorkspaceInvitationStatus,
  WorkspaceUsage,
};

use gotrue::params::{GenerateLinkParams, GenerateLinkType};
use shared_entity::dto::workspace_dto::{
  CreateWorkspaceMember, WorkspaceMemberChangeset, WorkspaceMemberInvitation,
};

use shared_entity::response::AppResponseError;
use sqlx::{types::uuid, PgPool};
use std::collections::HashMap;
use std::ops::DerefMut;
use std::sync::Arc;
use tracing::instrument;
use uuid::Uuid;
use workspace_template::document::get_started::GetStartedDocumentTemplate;

use crate::biz::collab::storage::CollabAccessControlStorage;
use crate::biz::user::user_init::initialize_workspace_for_user;

pub async fn delete_workspace_for_user(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  bucket_storage: &Arc<BucketStorage<BucketClientS3Impl>>,
) -> Result<(), AppResponseError> {
  // remove files from s3

  let blob_metadatas = get_all_workspace_blob_metadata(pg_pool, workspace_id)
    .await
    .context("Get all workspace blob metadata")?;

  for blob_metadata in blob_metadatas {
    bucket_storage
      .delete_blob(workspace_id, blob_metadata.file_id.as_str())
      .await
      .context("Delete blob from s3")?;
  }

  // remove from postgres
  delete_from_workspace(pg_pool, workspace_id).await?;

  // TODO: There can be a rare case where user uploads while workspace is being deleted.
  // We need some routine job to clean up these orphaned files.

  Ok(())
}

pub async fn create_workspace_for_user(
  pg_pool: &PgPool,
  workspace_access_control: &impl WorkspaceAccessControl,
  collab_storage: &Arc<CollabAccessControlStorage>,
  user_uuid: &Uuid,
  user_uid: i64,
  workspace_name: &str,
) -> Result<AFWorkspace, AppResponseError> {
  let mut txn = pg_pool.begin().await?;
  let new_workspace_row = insert_user_workspace(&mut txn, user_uuid, workspace_name).await?;

  workspace_access_control
    .insert_role(&user_uid, &new_workspace_row.workspace_id, AFRole::Owner)
    .await?;

  // add create initial collab for user
  initialize_workspace_for_user(
    user_uid,
    &new_workspace_row,
    &mut txn,
    vec![GetStartedDocumentTemplate],
    collab_storage,
  )
  .await?;

  let new_workspace = AFWorkspace::try_from(new_workspace_row)?;
  txn.commit().await?;
  Ok(new_workspace)
}

pub async fn patch_workspace(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  workspace_name: Option<&str>,
  workspace_icon: Option<&str>,
) -> Result<(), AppResponseError> {
  let mut tx = pg_pool.begin().await?;
  if let Some(workspace_name) = workspace_name {
    rename_workspace(&mut tx, workspace_id, workspace_name).await?;
  }
  if let Some(workspace_icon) = workspace_icon {
    change_workspace_icon(&mut tx, workspace_id, workspace_icon).await?;
  }
  tx.commit().await?;
  Ok(())
}

pub async fn get_all_user_workspaces(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
) -> Result<Vec<AFWorkspaceRow>, AppResponseError> {
  let workspaces = select_all_user_workspaces(pg_pool, user_uuid).await?;
  Ok(workspaces)
}

/// Returns the workspace with the given workspace_id and update the updated_at field of the
/// workspace.
pub async fn open_workspace(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
) -> Result<AFWorkspace, AppResponseError> {
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

pub async fn accept_workspace_invite(
  pg_pool: &PgPool,
  workspace_access_control: &impl WorkspaceAccessControl,
  user_uuid: &Uuid,
  invite_id: &Uuid,
) -> Result<(), AppError> {
  let mut txn = pg_pool.begin().await?;
  update_workspace_invitation_set_status_accepted(&mut txn, user_uuid, invite_id).await?;
  let inv = get_invitation_by_id(&mut txn, invite_id).await?;
  let invited_uid = inv
    .invitee_uid
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Invitee uid is missing for {:?}", inv)))?;
  workspace_access_control
    .insert_role(&invited_uid, &inv.workspace_id, inv.role)
    .await?;
  txn.commit().await?;
  Ok(())
}

#[instrument(level = "debug", skip_all, err)]
pub async fn invite_workspace_members(
  mailer: &Mailer,
  gotrue_admin: &GoTrueAdmin,
  pg_pool: &PgPool,
  gotrue_client: &gotrue::api::Client,
  inviter: &Uuid,
  workspace_id: &Uuid,
  invitations: Vec<WorkspaceMemberInvitation>,
) -> Result<(), AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to invite workspace members")?;
  let admin_token = gotrue_admin.token(gotrue_client).await?;

  for invitation in invitations {
    let name = database::user::select_name_from_email(pg_pool, &invitation.email).await?;
    let workspace_name =
      database::workspace::select_workspace_name_from_workspace_id(pg_pool, workspace_id)
        .await?
        .unwrap_or_default();
    let workspace_member_count =
      database::workspace::select_workspace_member_count_from_workspace_id(pg_pool, workspace_id)
        .await?
        .unwrap_or_default();

    // Generate a link such that when clicked, the user is added to the workspace.
    insert_workspace_invitation(
      &mut txn,
      workspace_id,
      inviter,
      invitation.email.as_str(),
      invitation.role,
    )
    .await?;

    let accept_url = gotrue_client
      .admin_generate_link(
        &admin_token,
        &GenerateLinkParams {
          type_: GenerateLinkType::MagicLink,
          email: invitation.email.clone(),
          redirect_to: "/web/accept-workspace".to_string(),
          ..Default::default()
        },
      )
      .await?
      .action_link;

    mailer
      .send_workspace_invite(
        invitation.email,
        name,
        workspace_name,
        workspace_member_count.to_string(),
        accept_url,
      )
      .await?;
  }

  txn
    .commit()
    .await
    .context("Commit transaction to invite workspace members")?;
  Ok(())
}

#[instrument(level = "debug", skip_all, err)]
pub async fn list_workspace_invitations_for_user(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  status: Option<AFWorkspaceInvitationStatus>,
) -> Result<Vec<AFWorkspaceInvitation>, AppError> {
  let invis = select_workspace_invitations_for_user(pg_pool, user_uuid, status).await?;
  Ok(invis)
}

/// Deprecated: use invitation workflow instead
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
#[instrument(level = "debug", skip_all, err)]
pub async fn add_workspace_members(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  workspace_id: &Uuid,
  members: Vec<CreateWorkspaceMember>,
  workspace_access_control: &impl WorkspaceAccessControl,
) -> Result<(), AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to insert workspace members")?;

  let mut role_by_uid = HashMap::new();
  for member in members.into_iter() {
    let access_level = AFAccessLevel::from(&member.role);
    let uid = select_uid_from_email(txn.deref_mut(), &member.email).await?;
    upsert_workspace_member_with_txn(&mut txn, workspace_id, &member.email, member.role.clone())
      .await?;
    upsert_collab_member_with_txn(uid, workspace_id.to_string(), &access_level, &mut txn).await?;
    role_by_uid.insert(uid, member.role);
  }

  for (uid, role) in role_by_uid {
    workspace_access_control
      .insert_role(&uid, workspace_id, role)
      .await?;
  }
  txn
    .commit()
    .await
    .context("Commit transaction to insert workspace members")?;

  Ok(())
}

// use in tests only
pub async fn add_workspace_members_db_only(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  workspace_id: &Uuid,
  members: Vec<CreateWorkspaceMember>,
) -> Result<(), AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to insert workspace members")?;

  for member in members.into_iter() {
    let access_level = match &member.role {
      AFRole::Owner => AFAccessLevel::FullAccess,
      AFRole::Member => AFAccessLevel::ReadAndWrite,
      AFRole::Guest => AFAccessLevel::ReadOnly,
    };

    let uid = select_uid_from_email(txn.deref_mut(), &member.email).await?;
    upsert_workspace_member_with_txn(&mut txn, workspace_id, &member.email, member.role.clone())
      .await?;
    upsert_collab_member_with_txn(uid, workspace_id.to_string(), &access_level, &mut txn).await?;
  }

  txn
    .commit()
    .await
    .context("Commit transaction to insert workspace members")?;

  Ok(())
}

pub async fn leave_workspace(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  user_uuid: &Uuid,
  workspace_access_control: &impl WorkspaceAccessControl,
) -> Result<(), AppResponseError> {
  let email = database::user::select_email_from_user_uuid(pg_pool, user_uuid).await?;
  remove_workspace_members(pg_pool, workspace_id, &[email], workspace_access_control).await
}

pub async fn remove_workspace_members(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  member_emails: &[String],
  workspace_access_control: &impl WorkspaceAccessControl,
) -> Result<(), AppResponseError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to delete workspace members")?;

  for email in member_emails {
    delete_workspace_members(&mut txn, workspace_id, email.as_str()).await?;
    if let Ok(uid) = select_uid_from_email(txn.deref_mut(), email)
      .await
      .map_err(AppResponseError::from)
    {
      workspace_access_control
        .remove_user_from_workspace(&uid, workspace_id)
        .await?;
    }
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
) -> Result<Vec<AFWorkspaceMemberRow>, AppResponseError> {
  Ok(select_workspace_member_list(pg_pool, workspace_id).await?)
}

pub async fn update_workspace_member(
  uid: &i64,
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  changeset: &WorkspaceMemberChangeset,
  workspace_access_control: &impl WorkspaceAccessControl,
) -> Result<(), AppError> {
  if let Some(role) = &changeset.role {
    upsert_workspace_member(pg_pool, workspace_id, &changeset.email, role.clone()).await?;
    workspace_access_control
      .insert_role(uid, workspace_id, role.clone())
      .await?;
  }

  Ok(())
}

pub async fn get_workspace_document_total_bytes(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
) -> Result<WorkspaceUsage, AppError> {
  let is_owner = select_user_is_workspace_owner(pg_pool, user_uuid, workspace_id).await?;
  if !is_owner {
    return Err(AppError::UserUnAuthorized(
      "User is not the owner of the workspace".to_string(),
    ));
  }

  let byte_count = select_workspace_total_collab_bytes(pg_pool, workspace_id).await?;
  Ok(WorkspaceUsage {
    total_document_size: byte_count,
  })
}
