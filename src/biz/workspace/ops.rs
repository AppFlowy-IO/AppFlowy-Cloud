use database_entity::dto::AFWorkspaceSettingsChange;
use std::collections::HashMap;

use anyhow::{anyhow, Context};
use redis::AsyncCommands;
use serde_json::json;
use sqlx::{types::uuid, PgPool};
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::Instant;
use tracing::instrument;
use uuid::Uuid;

use access_control::workspace::WorkspaceAccessControl;
use app_error::{AppError, ErrorCode};
use appflowy_collaborate::CollabMetrics;
use collab_stream::model::UpdateStreamMessage;
use database::collab::CollabStore;
use database::file::s3_client_impl::S3BucketStorage;
use database::pg_row::AFWorkspaceMemberRow;
use database::user::select_uid_from_email;
use database::workspace::*;
use database_entity::dto::{
  AFRole, AFWorkspace, AFWorkspaceInvitation, AFWorkspaceInvitationStatus, AFWorkspaceSettings,
  GlobalComment, Reaction, WorkspaceUsage,
};

use crate::biz::authentication::jwt::OptionalUserUuid;
use crate::biz::user::user_init::{
  create_user_awareness, create_workspace_collab, create_workspace_database_collab,
  initialize_workspace_for_user,
};
use crate::mailer::{AFCloudMailer, WorkspaceInviteMailerParam};
use crate::state::RedisConnectionManager;
use shared_entity::dto::workspace_dto::{
  CreateWorkspaceMember, WorkspaceMemberChangeset, WorkspaceMemberInvitation,
};
use shared_entity::response::AppResponseError;
use workspace_template::document::getting_started::GettingStartedTemplate;

pub(crate) const MAX_COMMENT_LENGTH: usize = 5000;

pub async fn delete_workspace_for_user(
  pg_pool: PgPool,
  mut connection_manager: RedisConnectionManager,
  workspace_id: Uuid,
  bucket_storage: Arc<S3BucketStorage>,
) -> Result<(), AppResponseError> {
  // remove files from s3
  bucket_storage
    .remove_dir(workspace_id.to_string().as_str())
    .await?;

  // remove from postgres
  delete_from_workspace(&pg_pool, &workspace_id).await?;
  let _: redis::Value = connection_manager
    .del(UpdateStreamMessage::stream_key(&workspace_id))
    .await
    .map_err(|err| AppResponseError::new(ErrorCode::Internal, err.to_string()))?;

  Ok(())
}

/// Create an empty workspace with default folder, workspace database and user awareness collab
/// object.
pub async fn create_empty_workspace(
  pg_pool: &PgPool,
  workspace_access_control: Arc<dyn WorkspaceAccessControl>,
  collab_storage: &Arc<dyn CollabStore>,
  collab_metrics: &CollabMetrics,
  user_uuid: &Uuid,
  user_uid: i64,
  workspace_name: &str,
) -> Result<AFWorkspace, AppResponseError> {
  let new_workspace_row =
    insert_user_workspace(pg_pool, user_uuid, workspace_name, "", false).await?;
  workspace_access_control
    .insert_role(&user_uid, &new_workspace_row.workspace_id, AFRole::Owner)
    .await?;
  let workspace_id = new_workspace_row.workspace_id;

  // create CollabType::Folder
  let mut txn = pg_pool.begin().await?;
  let start = Instant::now();
  create_workspace_collab(
    user_uid,
    workspace_id,
    workspace_name,
    collab_storage,
    &mut txn,
  )
  .await?;

  // create CollabType::WorkspaceDatabase
  if let Some(&database_storage_id) = new_workspace_row.database_storage_id.as_ref() {
    create_workspace_database_collab(
      workspace_id,
      &user_uid,
      database_storage_id,
      collab_storage,
      &mut txn,
      vec![],
    )
    .await?;
  }

  // create CollabType::UserAwareness
  create_user_awareness(&user_uid, user_uuid, workspace_id, collab_storage, &mut txn).await?;
  let new_workspace = AFWorkspace::try_from(new_workspace_row)?;
  txn.commit().await?;
  collab_metrics.observe_pg_tx(start.elapsed());
  Ok(new_workspace)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_workspace_for_user(
  pg_pool: &PgPool,
  workspace_access_control: Arc<dyn WorkspaceAccessControl>,
  collab_storage: &Arc<dyn CollabStore>,
  collab_metrics: &CollabMetrics,
  user_uuid: &Uuid,
  user_uid: i64,
  workspace_name: &str,
  workspace_icon: &str,
) -> Result<AFWorkspace, AppResponseError> {
  let new_workspace_row =
    insert_user_workspace(pg_pool, user_uuid, workspace_name, workspace_icon, true).await?;

  workspace_access_control
    .insert_role(&user_uid, &new_workspace_row.workspace_id, AFRole::Owner)
    .await?;

  // add create initial collab for user
  let mut txn = pg_pool.begin().await?;
  let start = Instant::now();
  initialize_workspace_for_user(
    user_uid,
    user_uuid,
    &new_workspace_row,
    &mut txn,
    vec![GettingStartedTemplate],
    collab_storage,
  )
  .await?;
  txn.commit().await?;
  collab_metrics.observe_pg_tx(start.elapsed());

  let new_workspace = AFWorkspace::try_from(new_workspace_row)?;
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

pub async fn get_all_user_workspaces(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  include_member_count: bool,
  include_role: bool,
  exclude_guest: bool,
) -> Result<Vec<AFWorkspace>, AppResponseError> {
  let workspaces = if exclude_guest {
    select_all_user_non_guest_workspaces(pg_pool, user_uuid).await?
  } else {
    select_all_user_workspaces(pg_pool, user_uuid).await?
  };
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
    let mut member_count_by_workspace_id =
      select_member_count_for_workspaces(pg_pool, &ids).await?;
    for workspace in workspaces.iter_mut() {
      if let Some(member_count) = member_count_by_workspace_id.remove(&workspace.workspace_id) {
        workspace.member_count = Some(member_count);
      }
    }
  }
  if include_role {
    let ids = workspaces
      .iter()
      .map(|row| row.workspace_id)
      .collect::<Vec<_>>();
    let mut roles_by_workspace_id = select_roles_for_workspaces(pg_pool, user_uuid, &ids).await?;
    for workspace in workspaces.iter_mut() {
      if let Some(role) = roles_by_workspace_id.remove(&workspace.workspace_id) {
        workspace.role = Some(role.clone());
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
  user_uid: i64,
  workspace_id: &Uuid,
) -> Result<AFWorkspace, AppResponseError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to open workspace")?;
  let row = select_workspace_with_count_and_role(txn.deref_mut(), workspace_id, user_uid).await?;
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
  workspace_access_control: Arc<dyn WorkspaceAccessControl>,
  user_uid: i64,
  user_uuid: &Uuid,
  invite_id: &Uuid,
) -> Result<(), AppError> {
  let mut txn = pg_pool.begin().await?;
  let inv = get_invitation_by_id(&mut txn, invite_id).await?;
  if let Some(invitee_uid) = inv.invitee_uid {
    if invitee_uid != user_uid {
      return Err(AppError::NotInviteeOfWorkspaceInvitation(format!(
        "User with uid {} is not the invitee for invite_id {}",
        user_uid, invite_id
      )));
    }
  }
  update_workspace_invitation_set_status_accepted(&mut txn, user_uuid, invite_id).await?;
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
#[allow(clippy::too_many_arguments)]
pub async fn invite_workspace_members(
  mailer: &AFCloudMailer,
  pg_pool: &PgPool,
  inviter: &Uuid,
  workspace_id: &Uuid,
  invitations: Vec<WorkspaceMemberInvitation>,
  appflowy_web_url: &str,
) -> Result<(), AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to invite workspace members")?;
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
    database::workspace::select_workspace_member_list_exclude_guest(pg_pool, workspace_id)
      .await?
      .into_iter()
      .map(|row| (row.email, row.role))
      .collect();
  let pending_invitations =
    database::workspace::select_workspace_pending_invitations(pg_pool, workspace_id).await?;

  // check if any of the invited users are already members of the workspace
  for invitation in &invitations {
    if workspace_members_by_email.contains_key(&invitation.email) {
      return Err(AppError::InvalidRequest(format!(
        "User with email {} is already a member of the workspace",
        invitation.email
      )));
    }
  }

  for invitation in invitations {
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
          &invitation.role,
        )
        .await?;
        invite_id
      },
      Some(invite_id) => {
        tracing::warn!("User already invited: {}", invitation.email);
        *invite_id
      },
    };

    // Generate a link such that when clicked, the user is added to the workspace.
    let accept_url = format!(
      "{}/accept-invitation?invited_id={}",
      appflowy_web_url, invite_id
    );

    if !invitation.skip_email_send {
      let cloned_mailer = mailer.clone();
      let email_sending = tokio::spawn(async move {
        cloned_mailer
          .send_workspace_invite(
            &invitation.email,
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
      });
      if invitation.wait_email_send {
        email_sending.await??;
      }
    } else {
      tracing::info!(
        "Skipping email send for workspace invite to {}",
        invitation.email
      );
    }
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

pub async fn get_workspace_invitations_for_user(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  invite_id: &Uuid,
) -> Result<AFWorkspaceInvitation, AppError> {
  let user_is_invitee =
    select_user_is_invitee_for_workspace_invitation(pg_pool, user_uuid, invite_id).await?;
  if !user_is_invitee {
    return Err(AppError::NotInviteeOfWorkspaceInvitation(format!(
      "User with uuid {} is not the invitee for invite_id {}",
      user_uuid, invite_id
    )));
  }
  let invitation = select_workspace_invitation_for_user(pg_pool, user_uuid, invite_id).await?;
  Ok(invitation)
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
    upsert_workspace_member_with_txn(&mut txn, workspace_id, &member.email, member.role.clone())
      .await?;
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
  workspace_access_control: Arc<dyn WorkspaceAccessControl>,
) -> Result<(), AppResponseError> {
  let email = database::user::select_email_from_user_uuid(pg_pool, user_uuid).await?;
  remove_workspace_members(pg_pool, workspace_id, &[email], workspace_access_control).await
}

pub async fn remove_workspace_members(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  member_emails: &[String],
  workspace_access_control: Arc<dyn WorkspaceAccessControl>,
) -> Result<(), AppResponseError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to delete workspace members")?;

  for email in member_emails {
    if let Ok(uid) = select_uid_from_email(txn.deref_mut(), email)
      .await
      .map_err(AppResponseError::from)
    {
      delete_workspace_members(&mut txn, workspace_id, email.as_str()).await?;
      workspace_access_control
        .remove_user_from_workspace(&uid, workspace_id)
        .await?;

      // TODO: Add permission cache invalidation for removed user
      // if let Some(realtime_server) = get_realtime_server_handle() {
      //   realtime_server.send_to_workspace(
      //     *workspace_id,
      //     InvalidateUserPermissions { uid }
      //   ).await;
      // }
    }
  }

  txn
    .commit()
    .await
    .context("Commit transaction to delete workspace members")?;
  Ok(())
}

pub async fn get_workspace_members_exclude_guest(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<Vec<AFWorkspaceMemberRow>, AppError> {
  select_workspace_member_list_exclude_guest(pg_pool, workspace_id).await
}

pub async fn get_workspace_member_optional(
  uid: i64,
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<Option<AFWorkspaceMemberRow>, AppError> {
  let member = select_workspace_member(pg_pool, uid, workspace_id).await?;
  Ok(member)
}

pub async fn get_workspace_member(
  uid: i64,
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<AFWorkspaceMemberRow, AppError> {
  let member = select_workspace_member(pg_pool, uid, workspace_id)
    .await?
    .ok_or(AppError::RecordNotFound("user does not exists".to_string()))?;
  Ok(member)
}

pub async fn get_workspace_owner(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<AFWorkspaceMemberRow, AppResponseError> {
  Ok(select_workspace_owner(pg_pool, workspace_id).await?)
}

pub async fn get_workspace_member_by_uuid(
  member_uuid: Uuid,
  pg_pool: &PgPool,
  workspace_id: Uuid,
) -> Result<AFWorkspaceMemberRow, AppResponseError> {
  Ok(select_workspace_member_by_uuid(pg_pool, member_uuid, workspace_id).await?)
}

pub async fn update_workspace_member(
  uid: &i64,
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  changeset: &WorkspaceMemberChangeset,
  workspace_access_control: Arc<dyn WorkspaceAccessControl>,
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
  workspace_id: &Uuid,
) -> Result<WorkspaceUsage, AppError> {
  let byte_count = select_workspace_total_collab_bytes(pg_pool, workspace_id).await?;
  Ok(WorkspaceUsage {
    total_document_size: byte_count,
  })
}

pub async fn get_workspace_settings(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<AFWorkspaceSettings, AppResponseError> {
  let settings = select_workspace_settings(pg_pool, workspace_id).await?;
  Ok(settings.unwrap_or_default())
}

pub async fn update_workspace_settings(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  change: AFWorkspaceSettingsChange,
) -> Result<AFWorkspaceSettings, AppResponseError> {
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

#[allow(clippy::too_many_arguments)]
pub async fn create_upload_task(
  uid: i64,
  task_id: Uuid,
  task: serde_json::Value,
  host: &str,
  workspace_id: &str,
  file_size: usize,
  presigned_url: Option<String>,
  redis_client: &RedisConnectionManager,
  pg_pool: &PgPool,
) -> Result<(), AppError> {
  // Insert the task into the database
  insert_import_task(
    uid,
    task_id,
    file_size as i64,
    workspace_id.to_string(),
    uid,
    Some(json!({"host": host})),
    presigned_url,
    pg_pool,
  )
  .await?;

  let _: () = redis_client
    .clone()
    .xadd("import_task_stream", "*", &[("task", task.to_string())])
    .await
    .map_err(|err| AppError::Internal(anyhow!("Failed to push task to Redis stream: {}", err)))?;

  Ok(())
}

pub async fn num_pending_task(uid: i64, pg_pool: &PgPool) -> Result<i64, AppError> {
  // Query to check for pending tasks for the given user ID
  let pending = ImportTaskState::Pending as i16;
  let query = "
        SELECT COUNT(*)
        FROM af_import_task
        WHERE uid = $1 AND status = $2
    ";

  // Execute the query and fetch the count
  let (count,): (i64,) = sqlx::query_as(query)
    .bind(uid)
    .bind(pending)
    .fetch_one(pg_pool)
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to query pending tasks: {:?}", e)))?;

  Ok(count)
}
