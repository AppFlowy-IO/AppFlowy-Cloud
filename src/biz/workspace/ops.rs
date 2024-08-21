use authentication::jwt::OptionalUserUuid;
use database_entity::dto::{AFWorkspaceSettingsChange, PublishCollabItem};
use std::collections::HashMap;

use database_entity::dto::PublishInfo;
use std::ops::DerefMut;
use std::sync::Arc;

use anyhow::Context;
use sqlx::{types::uuid, PgPool};
use tracing::instrument;
use uuid::Uuid;

use access_control::workspace::WorkspaceAccessControl;
use app_error::{AppError, ErrorCode};
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use database::collab::upsert_collab_member_with_txn;
use database::file::s3_client_impl::S3BucketStorage;
use database::pg_row::AFWorkspaceMemberRow;

use database::user::select_uid_from_email;
use database::workspace::*;
use database_entity::dto::{
  AFAccessLevel, AFRole, AFWorkspace, AFWorkspaceInvitation, AFWorkspaceInvitationStatus,
  AFWorkspaceSettings, GlobalComment, Reaction, WorkspaceUsage,
};
use gotrue::params::{GenerateLinkParams, GenerateLinkType};
use shared_entity::dto::workspace_dto::{
  CreateWorkspaceMember, WorkspaceMemberChangeset, WorkspaceMemberInvitation,
};
use shared_entity::response::AppResponseError;
use workspace_template::document::getting_started::GettingStartedTemplate;

use crate::biz::user::user_init::initialize_workspace_for_user;
use crate::mailer::{Mailer, WorkspaceInviteMailerParam};
use crate::state::GoTrueAdmin;

const MAX_COMMENT_LENGTH: usize = 5000;

pub async fn delete_workspace_for_user(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  bucket_storage: &Arc<S3BucketStorage>,
) -> Result<(), AppResponseError> {
  // remove files from s3
  bucket_storage
    .remove_dir(workspace_id.to_string().as_str())
    .await?;

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
    user_uuid,
    &new_workspace_row,
    &mut txn,
    vec![GettingStartedTemplate],
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

pub async fn set_workspace_namespace(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
  new_namespace: &str,
) -> Result<(), AppError> {
  check_workspace_owner(pg_pool, user_uuid, workspace_id).await?;
  check_workspace_namespace(new_namespace).await?;
  if select_workspace_publish_namespace_exists(pg_pool, workspace_id, new_namespace).await? {
    return Err(AppError::PublishNamespaceAlreadyTaken(
      "publish namespace is already taken".to_string(),
    ));
  };
  update_workspace_publish_namespace(pg_pool, workspace_id, new_namespace).await?;
  Ok(())
}

pub async fn get_workspace_publish_namespace(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<String, AppError> {
  select_workspace_publish_namespace(pg_pool, workspace_id).await
}

pub async fn publish_collabs(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  publisher_uuid: &Uuid,
  publish_items: &[PublishCollabItem<serde_json::Value, Vec<u8>>],
) -> Result<(), AppError> {
  for publish_item in publish_items {
    check_collab_publish_name(publish_item.meta.publish_name.as_str())?;
  }
  insert_or_replace_publish_collab_metas(pg_pool, workspace_id, publisher_uuid, publish_items)
    .await?;
  Ok(())
}

pub async fn get_published_collab(
  pg_pool: &PgPool,
  publish_namespace: &str,
  publish_name: &str,
) -> Result<serde_json::Value, AppError> {
  let metadata = select_publish_collab_meta(pg_pool, publish_namespace, publish_name).await?;
  Ok(metadata)
}

pub async fn get_published_collab_blob(
  pg_pool: &PgPool,
  publish_namespace: &str,
  publish_name: &str,
) -> Result<Vec<u8>, AppError> {
  select_published_collab_blob(pg_pool, publish_namespace, publish_name).await
}

pub async fn get_published_collab_info(
  pg_pool: &PgPool,
  view_id: &Uuid,
) -> Result<PublishInfo, AppError> {
  select_published_collab_info(pg_pool, view_id).await
}

pub async fn get_comments_on_published_view(
  pg_pool: &PgPool,
  view_id: &Uuid,
  optional_user_uuid: &OptionalUserUuid,
) -> Result<Vec<GlobalComment>, AppError> {
  let page_owner_uuid = select_owner_of_published_collab(pg_pool, view_id).await?;
  let comments = select_comments_for_published_view_ordered_by_recency(
    pg_pool,
    view_id,
    &optional_user_uuid.as_uuid(),
    &page_owner_uuid,
  )
  .await?;
  Ok(comments)
}

pub async fn create_comment_on_published_view(
  pg_pool: &PgPool,
  view_id: &Uuid,
  reply_comment_id: &Option<Uuid>,
  content: &str,
  user_uuid: &Uuid,
) -> Result<(), AppError> {
  if content.len() > MAX_COMMENT_LENGTH {
    return Err(AppError::StringLengthLimitReached(
      "comment content exceed limit".to_string(),
    ));
  }
  insert_comment_to_published_view(pg_pool, view_id, user_uuid, content, reply_comment_id).await?;
  Ok(())
}

pub async fn remove_comment_on_published_view(
  pg_pool: &PgPool,
  view_id: &Uuid,
  comment_id: &Uuid,
  user_uuid: &Uuid,
) -> Result<(), AppError> {
  check_if_user_is_allowed_to_delete_comment(pg_pool, user_uuid, view_id, comment_id).await?;
  update_comment_deletion_status(pg_pool, comment_id).await?;
  Ok(())
}

pub async fn get_reactions_on_published_view(
  pg_pool: &PgPool,
  view_id: &Uuid,
  comment_id: &Option<Uuid>,
) -> Result<Vec<Reaction>, AppError> {
  let reaction = match comment_id {
    Some(comment_id) => {
      select_reactions_for_comment_ordered_by_reaction_type_creation_time(pg_pool, comment_id)
        .await?
    },
    None => {
      select_reactions_for_published_view_ordered_by_reaction_type_creation_time(pg_pool, view_id)
        .await?
    },
  };
  Ok(reaction)
}

pub async fn create_reaction_on_comment(
  pg_pool: &PgPool,
  comment_id: &Uuid,
  view_id: &Uuid,
  reaction_type: &str,
  user_uuid: &Uuid,
) -> Result<(), AppError> {
  insert_reaction_on_comment(pg_pool, comment_id, view_id, user_uuid, reaction_type).await?;
  Ok(())
}

pub async fn remove_reaction_on_comment(
  pg_pool: &PgPool,
  comment_id: &Uuid,
  reaction_type: &str,
  user_uuid: &Uuid,
) -> Result<(), AppError> {
  delete_reaction_from_comment(pg_pool, comment_id, user_uuid, reaction_type).await?;
  Ok(())
}

pub async fn delete_published_workspace_collab(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  view_ids: &[Uuid],
  user_uuid: &Uuid,
) -> Result<(), AppError> {
  check_workspace_owner_or_publisher(pg_pool, user_uuid, workspace_id, view_ids).await?;
  delete_published_collabs(pg_pool, workspace_id, view_ids).await?;
  Ok(())
}

pub async fn get_all_user_workspaces(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  include_member_count: bool,
) -> Result<Vec<AFWorkspace>, AppResponseError> {
  let workspaces = select_all_user_workspaces(pg_pool, user_uuid).await?;
  let mut workspaces = workspaces
    .into_iter()
    .flat_map(|row| {
      let result = AFWorkspace::try_from(row);
      if let Err(err) = &result {
        tracing::error!("Failed to convert workspace row to AFWorkspace: {:?}", err);
      }
      result
    })
    .collect::<Vec<_>>();
  if include_member_count {
    let ids = workspaces
      .iter()
      .map(|row| row.workspace_id)
      .collect::<Vec<_>>();
    let member_count_by_workspace_id = select_member_count_for_workspaces(pg_pool, &ids).await?;
    for workspace in workspaces.iter_mut() {
      if let Some(member_count) = member_count_by_workspace_id.get(&workspace.workspace_id) {
        workspace.member_count = Some(*member_count);
      }
    }
  }

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

  let inviter_name = database::user::select_name_from_uuid(pg_pool, inviter).await?;
  let workspace_name =
    database::workspace::select_workspace_name_from_workspace_id(pg_pool, workspace_id)
      .await?
      .unwrap_or_default();
  let workspace_member_count =
    database::workspace::select_workspace_member_count_from_workspace_id(pg_pool, workspace_id)
      .await?
      .unwrap_or_default();
  let workspace_members_by_email: HashMap<_, _> =
    database::workspace::select_workspace_member_list(pg_pool, workspace_id)
      .await?
      .into_iter()
      .map(|row| (row.email, row.role))
      .collect();
  let pending_invitations =
    database::workspace::select_workspace_pending_invitations(pg_pool, workspace_id).await?;

  for invitation in invitations {
    if workspace_members_by_email.contains_key(&invitation.email) {
      tracing::warn!("User already in workspace: {}", invitation.email);
      continue;
    }

    let inviter_name = inviter_name.clone();
    let workspace_name = workspace_name.clone();
    let workspace_member_count = workspace_member_count.to_string();

    // use default icon until we have workspace icon
    let workspace_icon_url =
      "https://miro.medium.com/v2/resize:fit:2400/1*mTPfm7CwU31-tLhtLNkyJw.png".to_string();
    let user_icon_url =
      "https://cdn.pixabay.com/photo/2015/10/05/22/37/blank-profile-picture-973460_1280.png"
        .to_string();

    let invite_id = match pending_invitations.get(&invitation.email) {
      None => {
        // user is not invited yet
        let invite_id = uuid::Uuid::new_v4();
        insert_workspace_invitation(
          &mut txn,
          &invite_id,
          workspace_id,
          inviter,
          invitation.email.as_str(),
          invitation.role,
        )
        .await?;
        invite_id
      },
      Some(inv) => {
        tracing::warn!("User already invited: {}", invitation.email);
        *inv
      },
    };

    // Generate a link such that when clicked, the user is added to the workspace.
    let accept_url = gotrue_client
      .admin_generate_link(
        &admin_token,
        &GenerateLinkParams {
          type_: GenerateLinkType::MagicLink,
          email: invitation.email.clone(),
          redirect_to: format!(
            "/web/login-callback?action=accept_workspace_invite&workspace_invitation_id={}&workspace_name={}&workspace_icon={}&user_name={}&user_icon={}&workspace_member_count={}",
            invite_id, workspace_name,
            workspace_icon_url,
            inviter_name,
            user_icon_url,
            workspace_member_count,
          ),
          ..Default::default()
        },
      )
      .await?
      .action_link;

    // send email can be slow, so send email in background
    let cloned_mailer = mailer.clone();
    tokio::spawn(async move {
      if let Err(err) = cloned_mailer
        .send_workspace_invite(
          invitation.email,
          WorkspaceInviteMailerParam {
            user_icon_url,
            username: inviter_name,
            workspace_name,
            workspace_icon_url,
            workspace_member_count,
            accept_url,
          },
        )
        .await
      {
        tracing::error!("Failed to send workspace invite email: {:?}", err);
      };
    });
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
  workspace_id: &Uuid,
) -> Result<Vec<AFWorkspaceMemberRow>, AppResponseError> {
  Ok(select_workspace_member_list(pg_pool, workspace_id).await?)
}

pub async fn get_workspace_member(
  uid: &i64,
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<AFWorkspaceMemberRow, AppResponseError> {
  Ok(select_workspace_member(pg_pool, uid, workspace_id).await?)
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
  check_workspace_owner(pg_pool, user_uuid, workspace_id).await?;

  let byte_count = select_workspace_total_collab_bytes(pg_pool, workspace_id).await?;
  Ok(WorkspaceUsage {
    total_document_size: byte_count,
  })
}

pub async fn get_workspace_settings(
  pg_pool: &PgPool,
  workspace_access_control: &impl WorkspaceAccessControl,
  workspace_id: &Uuid,
  owner_uid: &i64,
) -> Result<AFWorkspaceSettings, AppResponseError> {
  let has_access = workspace_access_control
    .enforce_role(owner_uid, &workspace_id.to_string(), AFRole::Owner)
    .await?;

  if !has_access {
    return Err(AppResponseError::new(
      ErrorCode::UserUnAuthorized,
      "Only workspace owner can access workspace settings",
    ));
  }

  let settings = select_workspace_settings(pg_pool, workspace_id).await?;
  Ok(settings.unwrap_or_default())
}

pub async fn update_workspace_settings(
  pg_pool: &PgPool,
  workspace_access_control: &impl WorkspaceAccessControl,
  workspace_id: &Uuid,
  owner_uid: &i64,
  change: AFWorkspaceSettingsChange,
) -> Result<AFWorkspaceSettings, AppResponseError> {
  let has_access = workspace_access_control
    .enforce_role(owner_uid, &workspace_id.to_string(), AFRole::Owner)
    .await?;

  if !has_access {
    return Err(AppResponseError::new(
      ErrorCode::UserUnAuthorized,
      "Only workspace owner can edit workspace settings",
    ));
  }

  let mut tx = pg_pool.begin().await?;
  let mut setting = select_workspace_settings(tx.deref_mut(), workspace_id)
    .await?
    .unwrap_or_default();
  if let Some(disable_search_indexing) = change.disable_search_indexing {
    setting.disable_search_indexing = disable_search_indexing;
  }

  if let Some(ai_model) = change.ai_model {
    setting.ai_model = ai_model;
  }

  // Update the workspace settings in the database
  upsert_workspace_settings(&mut tx, workspace_id, &setting).await?;
  tx.commit().await?;
  Ok(setting)
}

async fn check_workspace_owner(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
) -> Result<(), AppError> {
  match select_user_is_workspace_owner(pg_pool, user_uuid, workspace_id).await? {
    true => Ok(()),
    false => Err(AppError::UserUnAuthorized(
      "User is not the owner of the workspace".to_string(),
    )),
  }
}

async fn check_workspace_namespace(new_namespace: &str) -> Result<(), AppError> {
  // Check len
  if new_namespace.len() < 8 {
    return Err(AppError::InvalidRequest(
      "Namespace must be at least 8 characters long".to_string(),
    ));
  }

  if new_namespace.len() > 64 {
    return Err(AppError::InvalidRequest(
      "Namespace must be at most 32 characters long".to_string(),
    ));
  }

  // Only contain alphanumeric characters and hyphens
  for c in new_namespace.chars() {
    if !c.is_alphanumeric() && c != '-' {
      return Err(AppError::InvalidRequest(
        "Namespace must only contain alphanumeric characters and hyphens".to_string(),
      ));
    }
  }

  // TODO: add more checks for reserved words

  Ok(())
}

async fn check_workspace_owner_or_publisher(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
  view_id: &[Uuid],
) -> Result<(), AppError> {
  let is_owner = select_user_is_workspace_owner(pg_pool, user_uuid, workspace_id).await?;
  if !is_owner {
    let is_publisher =
      select_user_is_collab_publisher_for_all_views(pg_pool, user_uuid, workspace_id, view_id)
        .await?;
    if !is_publisher {
      return Err(AppError::UserUnAuthorized(
        "User is not the owner of the workspace or the publisher of the document".to_string(),
      ));
    }
  }
  Ok(())
}

async fn check_if_user_is_allowed_to_delete_comment(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  view_id: &Uuid,
  comment_id: &Uuid,
) -> Result<(), AppError> {
  let is_allowed =
    select_user_is_allowed_to_delete_comment(pg_pool, user_uuid, view_id, comment_id).await?;
  if !is_allowed {
    return Err(AppError::UserUnAuthorized(
      "User is not allowed to delete this comment".to_string(),
    ));
  }
  Ok(())
}

fn check_collab_publish_name(publish_name: &str) -> Result<(), AppError> {
  // Check len
  if publish_name.len() > 128 {
    return Err(AppError::InvalidRequest(
      "Publish name must be at most 128 characters long".to_string(),
    ));
  }

  // Only contain alphanumeric characters and hyphens
  for c in publish_name.chars() {
    if !c.is_alphanumeric() && c != '-' {
      return Err(AppError::InvalidRequest(
        "Publish name must only contain alphanumeric characters and hyphens".to_string(),
      ));
    }
  }

  Ok(())
}
