use chrono::{DateTime, Utc};
use database_entity::dto::{
  AFRole, AFWorkspaceInvitation, AFWorkspaceInvitationStatus, AFWorkspaceSettings, GlobalComment,
  InvitationCodeInfo, MentionableWorkspaceMemberOrGuest, Reaction, WorkspaceMemberProfile,
};
use futures_util::stream::BoxStream;
use sqlx::{types::uuid, Executor, PgPool, Postgres, Transaction};
use std::{collections::HashMap, ops::DerefMut};
use tracing::{event, instrument};
use uuid::Uuid;

use crate::pg_row::{
  AFGlobalCommentRow, AFImportTask, AFPermissionRow, AFReactionRow, AFUserProfileRow,
  AFWebUserWithEmailColumn, AFWorkspaceInvitationMinimal, AFWorkspaceMemberPermRow,
  AFWorkspaceMemberRow, AFWorkspaceRow, AFWorkspaceRowWithMemberCountAndRole,
};
use crate::user::select_uid_from_email;
use app_error::AppError;

#[inline]
pub async fn delete_from_workspace(pg_pool: &PgPool, workspace_id: &Uuid) -> Result<(), AppError> {
  let res = sqlx::query!(
    r#"
      DELETE FROM public.af_workspace
      WHERE workspace_id = $1
    "#,
    workspace_id
  )
  .execute(pg_pool)
  .await?;

  if res.rows_affected() != 1 {
    tracing::error!(
      "Failed to delete workspace, workspace_id: {}, rows_affected: {}",
      workspace_id,
      res.rows_affected()
    );
  }

  Ok(())
}

#[inline]
pub async fn insert_user_workspace(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_name: &str,
  workspace_icon: &str,
  is_initialized: bool,
) -> Result<AFWorkspaceRow, AppError> {
  let workspace = sqlx::query_as!(
    AFWorkspaceRow,
    r#"
    WITH new_workspace AS (
      INSERT INTO public.af_workspace (owner_uid, workspace_name, icon, is_initialized)
      VALUES ((SELECT uid FROM public.af_user WHERE uuid = $1), $2, $3, $4)
      RETURNING *
    )
    SELECT
      workspace_id,
      database_storage_id,
      owner_uid,
      owner_profile.name AS owner_name,
      owner_profile.email AS owner_email,
      new_workspace.created_at,
      workspace_type,
      new_workspace.deleted_at,
      workspace_name,
      icon
    FROM new_workspace
    JOIN public.af_user AS owner_profile ON new_workspace.owner_uid = owner_profile.uid;
    "#,
    user_uuid,
    workspace_name,
    workspace_icon,
    is_initialized,
  )
  .fetch_one(pg_pool)
  .await?;

  Ok(workspace)
}

#[inline]
pub async fn rename_workspace(
  tx: &mut Transaction<'_, sqlx::Postgres>,
  workspace_id: &Uuid,
  new_workspace_name: &str,
) -> Result<(), AppError> {
  let res = sqlx::query!(
    r#"
      UPDATE public.af_workspace
      SET workspace_name = $1
      WHERE workspace_id = $2
    "#,
    new_workspace_name,
    workspace_id,
  )
  .execute(tx.deref_mut())
  .await?;

  if res.rows_affected() != 1 {
    tracing::error!("Failed to rename workspace, workspace_id: {}", workspace_id);
  }
  Ok(())
}

#[inline]
pub async fn change_workspace_icon(
  tx: &mut Transaction<'_, sqlx::Postgres>,
  workspace_id: &Uuid,
  icon: &str,
) -> Result<(), AppError> {
  let res = sqlx::query!(
    r#"
      UPDATE public.af_workspace
      SET icon = $1
      WHERE workspace_id = $2
    "#,
    icon,
    workspace_id,
  )
  .execute(tx.deref_mut())
  .await?;

  if res.rows_affected() != 1 {
    tracing::error!(
      "Failed to change workspace icon, workspace_id: {}",
      workspace_id
    );
  }
  Ok(())
}

/// Checks whether a user, identified by a UUID, is an 'Owner' of a workspace, identified by its
/// workspace_id.
#[inline]
pub async fn select_user_is_workspace_owner(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_uuid: &Uuid,
) -> Result<bool, AppError> {
  let exists = sqlx::query_scalar!(
    r#"
  SELECT EXISTS(
    SELECT 1
    FROM public.af_workspace_member
      JOIN af_roles ON af_workspace_member.role_id = af_roles.id
    WHERE workspace_id = $1
    AND af_workspace_member.uid = (
      SELECT uid FROM public.af_user WHERE uuid = $2
    )
    AND af_roles.name = 'Owner'
  ) AS "exists";
  "#,
    workspace_uuid,
    user_uuid
  )
  .fetch_one(pg_pool)
  .await?;

  Ok(exists.unwrap_or(false))
}

pub async fn select_user_is_allowed_to_delete_comment(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  view_id: &Uuid,
  comment_id: &Uuid,
) -> Result<bool, AppError> {
  let is_publisher_for_view = sqlx::query_scalar!(
    r#"
    SELECT EXISTS(
      SELECT true
      FROM af_published_collab
      WHERE view_id = $1
        AND published_by = (SELECT uid FROM af_user WHERE uuid = $2)
      UNION ALL
      SELECT true
      FROM af_published_view_comment
      WHERE view_id = $1
        AND comment_id = $3
        AND created_by = (SELECT uid FROM af_user WHERE uuid = $2)
    ) AS "exists";
    "#,
    view_id,
    user_uuid,
    comment_id,
  )
  .fetch_one(pg_pool)
  .await?;

  Ok(is_publisher_for_view.unwrap_or(false))
}

#[inline]
pub async fn select_user_role<'a, E: Executor<'a, Database = Postgres>>(
  exectuor: E,
  uid: &i64,
  workspace_uuid: &Uuid,
) -> Result<AFRole, AppError> {
  let row = sqlx::query_scalar!(
    r#"
     SELECT role_id FROM af_workspace_member
     WHERE workspace_id = $1 AND uid = $2
    "#,
    workspace_uuid,
    uid
  )
  .fetch_one(exectuor)
  .await?;
  Ok(AFRole::from(row))
}

#[inline]
pub async fn upsert_workspace_member_with_txn(
  txn: &mut Transaction<'_, sqlx::Postgres>,
  workspace_id: &uuid::Uuid,
  member_email: &str,
  role: AFRole,
) -> Result<(), AppError> {
  let role_id: i32 = role.into();
  sqlx::query!(
    r#"
      INSERT INTO public.af_workspace_member (workspace_id, uid, role_id)
      SELECT $1, af_user.uid, $3
      FROM public.af_user
      WHERE
        af_user.email = $2
      ON CONFLICT (workspace_id, uid)
      DO NOTHING;
    "#,
    workspace_id,
    member_email,
    role_id
  )
  .execute(txn.deref_mut())
  .await?;

  Ok(())
}

#[inline]
pub async fn insert_workspace_invitation(
  txn: &mut Transaction<'_, sqlx::Postgres>,
  invite_id: &uuid::Uuid,
  workspace_id: &uuid::Uuid,
  inviter_uuid: &Uuid,
  invitee_email: &str,
  invitee_role: &AFRole,
) -> Result<(), AppError> {
  let role_id: i32 = invitee_role.into();
  sqlx::query!(
    r#"
      INSERT INTO public.af_workspace_invitation (
          id,
          workspace_id,
          inviter,
          invitee_email,
          role_id
      )
      VALUES (
        $1,
        $2,
        (SELECT uid FROM public.af_user WHERE uuid = $3),
        $4,
        $5
      )
    "#,
    invite_id,
    workspace_id,
    inviter_uuid,
    invitee_email,
    role_id
  )
  .execute(txn.deref_mut())
  .await?;

  Ok(())
}

pub async fn update_workspace_invitation_set_status_accepted(
  txn: &mut Transaction<'_, sqlx::Postgres>,
  invitee_uuid: &Uuid,
  invite_id: &Uuid,
) -> Result<(), AppError> {
  let res = sqlx::query_scalar!(
    r#"
    UPDATE public.af_workspace_invitation
    SET status = 1
    WHERE LOWER(invitee_email) = (SELECT LOWER(email) FROM public.af_user WHERE uuid = $1)
      AND id = $2
      AND status = 0
    "#,
    invitee_uuid,
    invite_id,
  )
  .execute(txn.deref_mut())
  .await?;
  match res.rows_affected() {
    0 => Err(AppError::RecordNotFound(format!(
      "Invitation not found, invitee_uuid: {}, invite_id: {}",
      invitee_uuid, invite_id
    ))),
    1 => Ok(()),
    x => Err(
      anyhow::anyhow!(
        "Expected 1 row to be affected, but {} rows were affected",
        x
      )
      .into(),
    ),
  }
}

pub async fn get_invitation_by_id(
  txn: &mut Transaction<'_, sqlx::Postgres>,
  invite_id: &Uuid,
) -> Result<AFWorkspaceInvitationMinimal, AppError> {
  let res = sqlx::query_as!(
    AFWorkspaceInvitationMinimal,
    r#"
    SELECT
        workspace_id,
        inviter AS inviter_uid,
        (SELECT uid FROM public.af_user WHERE LOWER(email) = LOWER(invitee_email)) AS invitee_uid,
        status,
        role_id AS role
    FROM
    public.af_workspace_invitation
    WHERE id = $1
    "#,
    invite_id,
  )
  .fetch_one(txn.deref_mut())
  .await?;

  Ok(res)
}

#[inline]
pub async fn select_workspace_invitations_for_user(
  pg_pool: &PgPool,
  invitee_uuid: &Uuid,
  status_filter: Option<AFWorkspaceInvitationStatus>,
) -> Result<Vec<AFWorkspaceInvitation>, AppError> {
  let res = sqlx::query_as!(
    AFWorkspaceInvitation,
    r#"
      SELECT
        i.id AS invite_id,
        i.workspace_id,
        w.workspace_name,
        u_inviter.email AS inviter_email,
        u_inviter.name AS inviter_name,
        i.status,
        i.updated_at,
        u_inviter.metadata->>'icon_url' AS inviter_icon,
        w.icon AS workspace_icon,
        (SELECT COUNT(*) FROM public.af_workspace_member m WHERE m.workspace_id = i.workspace_id) AS member_count
      FROM
        public.af_workspace_invitation i
        JOIN public.af_workspace w ON i.workspace_id = w.workspace_id
        JOIN public.af_user u_inviter ON i.inviter = u_inviter.uid
        JOIN public.af_user u_invitee ON u_invitee.uuid = $1
      WHERE
        LOWER(i.invitee_email) = LOWER(u_invitee.email)
        AND ($2::SMALLINT IS NULL OR i.status = $2);
    "#,
    invitee_uuid,
    status_filter.map(|s| s as i16)
  )
  .fetch_all(pg_pool)
  .await?;
  Ok(res)
}

#[inline]
pub async fn select_workspace_invitation_for_user(
  pg_pool: &PgPool,
  invitee_uuid: &Uuid,
  invite_id: &Uuid,
) -> Result<AFWorkspaceInvitation, AppError> {
  let res = sqlx::query_as!(
    AFWorkspaceInvitation,
    r#"
      SELECT
        i.id AS invite_id,
        i.workspace_id,
        w.workspace_name,
        u_inviter.email AS inviter_email,
        u_inviter.name AS inviter_name,
        i.status,
        i.updated_at,
        u_inviter.metadata->>'icon_url' AS inviter_icon,
        w.icon AS workspace_icon,
        (SELECT COUNT(*) FROM public.af_workspace_member m WHERE m.workspace_id = i.workspace_id) AS member_count
      FROM
        public.af_workspace_invitation i
        JOIN public.af_workspace w ON i.workspace_id = w.workspace_id
        JOIN public.af_user u_inviter ON i.inviter = u_inviter.uid
        JOIN public.af_user u_invitee ON u_invitee.uuid = $1
      WHERE
        LOWER(i.invitee_email) = LOWER(u_invitee.email)
        AND i.id = $2;
    "#,
    invitee_uuid,
    invite_id,
  )
  .fetch_one(pg_pool)
  .await?;
  Ok(res)
}

#[inline]
#[instrument(level = "trace", skip(pool, email, role), err)]
pub async fn upsert_workspace_member(
  pool: &PgPool,
  workspace_id: &Uuid,
  email: &str,
  role: AFRole,
) -> Result<(), sqlx::Error> {
  event!(
    tracing::Level::TRACE,
    "update workspace member: workspace_id:{}, uid {:?}, role:{:?}",
    workspace_id,
    select_uid_from_email(pool, email).await,
    role
  );

  let role_id: i32 = role.into();
  sqlx::query!(
    r#"
        UPDATE af_workspace_member
        SET
            role_id = $1
        WHERE workspace_id = $2 AND uid = (
            SELECT uid FROM af_user WHERE email = $3
        )
        "#,
    role_id,
    workspace_id,
    email
  )
  .execute(pool)
  .await?;

  Ok(())
}

#[inline]
pub async fn delete_workspace_members(
  txn: &mut Transaction<'_, sqlx::Postgres>,
  workspace_id: &Uuid,
  member_email: &str,
) -> Result<(), AppError> {
  let is_owner = sqlx::query_scalar!(
    r#"
  SELECT EXISTS (
    SELECT 1
    FROM public.af_workspace
    WHERE
        workspace_id = $1
        AND owner_uid = (
            SELECT uid FROM public.af_user WHERE email = $2
        )
   ) AS "is_owner";
  "#,
    workspace_id,
    member_email
  )
  .fetch_one(txn.deref_mut())
  .await?
  .unwrap_or(false);

  if is_owner {
    return Err(AppError::NotEnoughPermissions);
  }

  sqlx::query!(
    r#"
    DELETE FROM public.af_workspace_member
    WHERE
    workspace_id = $1
    AND uid = (
        SELECT uid FROM public.af_user WHERE email = $2
    )
    -- Ensure the user to be deleted is not the original owner.
    -- 1. TODO(nathan): User must transfer ownership to another user first.
    -- 2. User must have at least one workspace
    AND uid <> (
        SELECT owner_uid FROM public.af_workspace WHERE workspace_id = $1
    );
    "#,
    workspace_id,
    member_email,
  )
  .execute(txn.deref_mut())
  .await?;

  Ok(())
}

pub fn select_workspace_member_perm_stream(
  pg_pool: &PgPool,
) -> BoxStream<'_, sqlx::Result<AFWorkspaceMemberPermRow>> {
  sqlx::query_as!(
    AFWorkspaceMemberPermRow,
    "SELECT uid, role_id as role, workspace_id FROM af_workspace_member"
  )
  .fetch(pg_pool)
}

pub async fn select_email_belongs_to_a_workspace_member(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  email: &str,
) -> Result<bool, AppError> {
  let exists = sqlx::query_scalar!(
    r#"
    SELECT EXISTS(
      SELECT 1
      FROM public.af_workspace_member
      JOIN public.af_user ON af_workspace_member.uid = af_user.uid
      WHERE af_workspace_member.workspace_id = $1
      AND LOWER(af_user.email) = LOWER($2)
    ) AS "exists";
    "#,
    workspace_id,
    email
  )
  .fetch_one(pg_pool)
  .await?;

  Ok(exists.unwrap_or(false))
}

/// returns a list of workspace members, sorted by their creation time.
#[inline]
pub async fn select_workspace_member_list_exclude_guest(
  pg_pool: &PgPool,
  workspace_id: &uuid::Uuid,
) -> Result<Vec<AFWorkspaceMemberRow>, AppError> {
  let members = sqlx::query_as!(
    AFWorkspaceMemberRow,
    r#"
    SELECT
      af_user.uid,
      af_user.name,
      af_user.email,
      af_user.metadata ->> 'icon_url' AS avatar_url,
      af_workspace_member.role_id AS role,
      af_workspace_member.created_at
    FROM public.af_workspace_member
        JOIN public.af_user ON af_workspace_member.uid = af_user.uid
    WHERE af_workspace_member.workspace_id = $1
    AND role_id != $2
    ORDER BY af_workspace_member.created_at ASC;
    "#,
    workspace_id,
    AFRole::Guest as i32,
  )
  .fetch_all(pg_pool)
  .await?;
  Ok(members)
}

#[inline]
pub async fn select_workspace_member<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  uid: i64,
  workspace_id: &Uuid,
) -> Result<Option<AFWorkspaceMemberRow>, AppError> {
  let member = sqlx::query_as!(
    AFWorkspaceMemberRow,
    r#"
    SELECT
      af_user.uid,
      af_user.name,
      af_user.email,
      af_user.metadata ->> 'icon_url' AS avatar_url,
      af_workspace_member.role_id AS role,
      af_workspace_member.created_at
    FROM public.af_workspace_member
      JOIN public.af_user ON af_workspace_member.uid = af_user.uid
    WHERE af_workspace_member.workspace_id = $1
    AND af_workspace_member.uid = $2
    "#,
    workspace_id,
    uid,
  )
  .fetch_optional(executor)
  .await?;
  Ok(member)
}

pub async fn select_workspace_owner<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
) -> Result<AFWorkspaceMemberRow, AppError> {
  let member = sqlx::query_as!(
    AFWorkspaceMemberRow,
    r#"
    SELECT
      af_user.uid,
      af_user.name,
      af_user.email,
      af_user.metadata ->> 'icon_url' AS avatar_url,
      af_workspace_member.role_id AS role,
      af_workspace_member.created_at
    FROM public.af_workspace_member
    JOIN public.af_workspace USING(workspace_id)
    JOIN public.af_user ON af_workspace.owner_uid = af_user.uid
    WHERE af_workspace_member.workspace_id = $1
    "#,
    workspace_id,
  )
  .fetch_one(executor)
  .await?;
  Ok(member)
}

#[inline]
pub async fn select_workspace_member_by_uuid<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  uuid: Uuid,
  workspace_id: Uuid,
) -> Result<AFWorkspaceMemberRow, AppError> {
  let member = sqlx::query_as!(
    AFWorkspaceMemberRow,
    r#"
    SELECT
      af_user.uid,
      af_user.name,
      af_user.email,
      af_user.metadata ->> 'icon_url' AS avatar_url,
      af_workspace_member.role_id AS role,
      af_workspace_member.created_at
    FROM public.af_workspace_member
      JOIN public.af_user ON af_workspace_member.uid = af_user.uid
    WHERE af_workspace_member.workspace_id = $1
    AND af_user.uuid = $2
    "#,
    workspace_id,
    uuid,
  )
  .fetch_one(executor)
  .await?;
  Ok(member)
}

#[inline]
pub async fn select_user_profile<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
) -> Result<Option<AFUserProfileRow>, AppError> {
  let user_profile = sqlx::query_as!(
    AFUserProfileRow,
    r#"
      WITH af_user_row AS (
        SELECT * FROM af_user WHERE uuid = $1
      )
      SELECT
        af_user_row.uid,
        af_user_row.uuid,
        af_user_row.email,
        af_user_row.password,
        af_user_row.name,
        af_user_row.metadata,
        af_user_row.encryption_sign,
        af_user_row.deleted_at,
        af_user_row.updated_at,
        af_user_row.created_at,
       (
         SELECT af_workspace_member.workspace_id
         FROM af_workspace_member
         JOIN af_workspace
           ON af_workspace_member.workspace_id = af_workspace.workspace_id
         WHERE af_workspace_member.uid = af_user_row.uid
           AND COALESCE(af_workspace.is_initialized, true) = true
         ORDER BY af_workspace_member.updated_at DESC
         LIMIT 1
       ) AS latest_workspace_id
      FROM af_user_row
    "#,
    user_uuid
  )
  .fetch_optional(executor)
  .await?;
  Ok(user_profile)
}

#[inline]
pub async fn select_workspace<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
) -> Result<AFWorkspaceRow, AppError> {
  let workspace = sqlx::query_as!(
    AFWorkspaceRow,
    r#"
      SELECT
        workspace_id,
        database_storage_id,
        owner_uid,
        owner_profile.name as owner_name,
        owner_profile.email as owner_email,
        af_workspace.created_at,
        workspace_type,
        af_workspace.deleted_at,
        workspace_name,
        icon
      FROM public.af_workspace
      JOIN public.af_user owner_profile ON af_workspace.owner_uid = owner_profile.uid
      WHERE af_workspace.workspace_id = $1
        AND COALESCE(af_workspace.is_initialized, true) = true;
    "#,
    workspace_id
  )
  .fetch_one(executor)
  .await?;
  Ok(workspace)
}

#[inline]
pub async fn select_workspace_with_count_and_role<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
  uid: i64,
) -> Result<AFWorkspaceRowWithMemberCountAndRole, AppError> {
  let workspace = sqlx::query_as!(
    AFWorkspaceRowWithMemberCountAndRole,
    r#"
      WITH workspace_member_count AS (
        SELECT
          workspace_id,
          COUNT(*) AS member_count
        FROM af_workspace_member
        WHERE workspace_id = $1 AND role_id != $3
        GROUP BY workspace_id
      )

      SELECT
        af_workspace.workspace_id,
        database_storage_id,
        owner_uid,
        owner_profile.name as owner_name,
        owner_profile.email as owner_email,
        af_workspace.created_at,
        workspace_type,
        af_workspace.deleted_at,
        workspace_name,
        icon,
        workspace_member_count.member_count AS "member_count!",
        role_id AS "role!"
      FROM public.af_workspace
      JOIN public.af_user owner_profile ON af_workspace.owner_uid = owner_profile.uid
      JOIN af_workspace_member ON (af_workspace.workspace_id = af_workspace_member.workspace_id
        AND af_workspace_member.uid = $2)
      JOIN workspace_member_count ON af_workspace.workspace_id = workspace_member_count.workspace_id
      WHERE af_workspace.workspace_id = $1
        AND COALESCE(af_workspace.is_initialized, true) = true;
    "#,
    workspace_id,
    uid,
    AFRole::Guest as i32, // Exclude guests from member count
  )
  .fetch_one(executor)
  .await?;
  Ok(workspace)
}

#[inline]
pub async fn select_workspace_database_storage_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &str,
) -> Result<Uuid, AppError> {
  let workspace_id = Uuid::parse_str(workspace_id)?;
  let result = sqlx::query!(
    r#"
        SELECT
            database_storage_id
        FROM public.af_workspace
        WHERE workspace_id = $1
        "#,
    workspace_id
  )
  .fetch_one(executor)
  .await?;

  Ok(result.database_storage_id)
}

#[inline]
pub async fn update_updated_at_of_workspace<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
       UPDATE af_workspace_member
       SET updated_at = CURRENT_TIMESTAMP
       WHERE uid = (SELECT uid FROM public.af_user WHERE uuid = $1) AND workspace_id = $2;
    "#,
    user_uuid,
    workspace_id
  )
  .execute(executor)
  .await?;
  Ok(())
}

#[inline]
pub async fn update_updated_at_of_workspace_with_uid<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  uid: i64,
  workspace_id: &Uuid,
  current_timestamp: DateTime<Utc>,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
        UPDATE af_workspace_member
        SET updated_at = $3
        WHERE uid = $1
        AND workspace_id = $2;
        "#,
    uid,
    workspace_id,
    current_timestamp
  )
  .execute(executor)
  .await?;

  Ok(())
}

/// Returns a list of workspaces that the user is part of.
/// User may be guest.
#[inline]
pub async fn select_all_user_workspaces<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
) -> Result<Vec<AFWorkspaceRowWithMemberCountAndRole>, AppError> {
  let workspaces = sqlx::query_as!(
    AFWorkspaceRowWithMemberCountAndRole,
    r#"
      WITH user_workspace_id AS (
        SELECT workspace_id
        FROM af_workspace_member
        JOIN af_user ON af_workspace_member.uid = af_user.uid
        WHERE af_user.uuid = $1
      ),
      workspace_member_count AS (
        SELECT
          workspace_id,
          COUNT(*) AS member_count
        FROM af_workspace_member
        JOIN user_workspace_id USING (workspace_id)
        WHERE role_id != $2
        GROUP BY workspace_id
      )

      SELECT
        w.workspace_id,
        w.database_storage_id,
        w.owner_uid,
        u.name AS owner_name,
        u.email AS owner_email,
        w.created_at,
        w.workspace_type,
        w.deleted_at,
        w.workspace_name,
        w.icon,
        wmc.member_count AS "member_count!",
        wm.role_id AS "role!"
      FROM af_workspace w
      JOIN af_workspace_member wm ON w.workspace_id = wm.workspace_id
      JOIN public.af_user u ON w.owner_uid = u.uid
      JOIN workspace_member_count wmc ON w.workspace_id = wmc.workspace_id
      WHERE wm.uid = (
         SELECT uid FROM public.af_user WHERE uuid = $1
      )
      AND COALESCE(w.is_initialized, true) = true;
    "#,
    user_uuid,
    AFRole::Guest as i32, // Exclude guests from member count
  )
  .fetch_all(executor)
  .await?;
  Ok(workspaces)
}

/// Returns a list of workspaces that the user is part of.
/// User must be at least member.
#[inline]
pub async fn select_all_user_non_guest_workspaces<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
) -> Result<Vec<AFWorkspaceRowWithMemberCountAndRole>, AppError> {
  let workspaces = sqlx::query_as!(
    AFWorkspaceRowWithMemberCountAndRole,
    r#"
      WITH user_workspace_id AS (
        SELECT workspace_id
        FROM af_workspace_member
        JOIN af_user ON af_workspace_member.uid = af_user.uid
        WHERE af_user.uuid = $1
      ),
      workspace_member_count AS (
        SELECT
          workspace_id,
          COUNT(*) AS member_count
        FROM af_workspace_member
        JOIN user_workspace_id USING (workspace_id)
        WHERE role_id != $2
        GROUP BY workspace_id
      )

      SELECT
        w.workspace_id,
        w.database_storage_id,
        w.owner_uid,
        u.name AS owner_name,
        u.email AS owner_email,
        w.created_at,
        w.workspace_type,
        w.deleted_at,
        w.workspace_name,
        w.icon,
        wmc.member_count AS "member_count!",
        wm.role_id AS "role!"
      FROM af_workspace w
      JOIN af_workspace_member wm ON w.workspace_id = wm.workspace_id
      JOIN public.af_user u ON w.owner_uid = u.uid
      JOIN workspace_member_count wmc ON w.workspace_id = wmc.workspace_id
      WHERE wm.uid = (
         SELECT uid FROM public.af_user WHERE uuid = $1
      )
      AND wm.role_id != $2
      AND COALESCE(w.is_initialized, true) = true;
    "#,
    user_uuid,
    AFRole::Guest as i32,
  )
  .fetch_all(executor)
  .await?;
  Ok(workspaces)
}

/// Returns a list of workspace ids that the user is owner of.
#[inline]
pub async fn select_user_owned_workspaces_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
) -> Result<Vec<Uuid>, AppError> {
  let workspace_ids = sqlx::query_scalar!(
    r#"
      SELECT workspace_id
      FROM af_workspace
      WHERE owner_uid = (SELECT uid FROM public.af_user WHERE uuid = $1)
    "#,
    user_uuid
  )
  .fetch_all(executor)
  .await?;
  Ok(workspace_ids)
}

pub async fn insert_workspace_ids_to_deleted_table<'a, E>(
  executor: E,
  workspace_ids: Vec<Uuid>,
) -> Result<(), AppError>
where
  E: Executor<'a, Database = Postgres>,
{
  if workspace_ids.is_empty() {
    return Ok(());
  }

  let query = "INSERT INTO public.af_workspace_deleted (workspace_id) SELECT unnest($1::uuid[])";
  sqlx::query(query)
    .bind(workspace_ids)
    .execute(executor)
    .await?;
  Ok(())
}

pub async fn update_workspace_status<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
  is_initialized: bool,
) -> Result<(), AppError> {
  let res = sqlx::query!(
    r#"
    UPDATE public.af_workspace
    SET is_initialized = $2
    WHERE workspace_id = $1
    "#,
    workspace_id,
    is_initialized
  )
  .execute(executor)
  .await?;

  if res.rows_affected() != 1 {
    tracing::error!(
      "Failed to update workspace status, workspace_id: {}",
      workspace_id
    );
  }
  Ok(())
}

pub async fn select_member_count_for_workspaces<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_ids: &[Uuid],
) -> Result<HashMap<Uuid, i64>, AppError> {
  let query_res = sqlx::query!(
    r#"
      SELECT workspace_id, COUNT(*) AS member_count
      FROM af_workspace_member
      WHERE workspace_id = ANY($1) AND role_id != $2
      GROUP BY workspace_id
    "#,
    workspace_ids,
    AFRole::Guest as i32, // Exclude guests from member count
  )
  .fetch_all(executor)
  .await?;

  let mut ret = HashMap::with_capacity(workspace_ids.len());
  for row in query_res {
    let count = match row.member_count {
      Some(c) => c,
      None => continue,
    };
    ret.insert(row.workspace_id, count);
  }

  Ok(ret)
}

pub async fn select_roles_for_workspaces(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_ids: &[Uuid],
) -> Result<HashMap<Uuid, AFRole>, AppError> {
  let query_res = sqlx::query!(
    r#"
      SELECT workspace_id, role_id
      FROM af_workspace_member
      WHERE workspace_id = ANY($1)
        AND uid = (SELECT uid FROM public.af_user WHERE uuid = $2)
    "#,
    workspace_ids,
    user_uuid,
  )
  .fetch_all(pg_pool)
  .await?;

  let mut ret = HashMap::with_capacity(workspace_ids.len());
  for row in query_res {
    let role = AFRole::from(row.role_id);
    ret.insert(row.workspace_id, role);
  }

  Ok(ret)
}

pub async fn select_permission(
  pool: &PgPool,
  permission_id: &i64,
) -> Result<Option<AFPermissionRow>, AppError> {
  let permission = sqlx::query_as!(
    AFPermissionRow,
    r#"
      SELECT * FROM public.af_permissions WHERE id = $1
    "#,
    *permission_id as i32
  )
  .fetch_optional(pool)
  .await?;
  Ok(permission)
}

pub async fn select_permission_from_role_id(
  pool: &PgPool,
  role_id: &i64,
) -> Result<Option<AFPermissionRow>, AppError> {
  let permission = sqlx::query_as!(
    AFPermissionRow,
    r#"
    SELECT p.id, p.name, p.access_level, p.description FROM af_permissions p
    JOIN af_role_permissions rp ON p.id = rp.permission_id
    WHERE rp.role_id = $1
    "#,
    *role_id as i32
  )
  .fetch_optional(pool)
  .await?;
  Ok(permission)
}

pub async fn select_workspace_total_collab_bytes(
  pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<i64, AppError> {
  let sum = sqlx::query_scalar!(
    r#"
    SELECT SUM(len) FROM af_collab WHERE workspace_id = $1
    "#,
    workspace_id
  )
  .fetch_one(pool)
  .await?;

  match sum {
    Some(s) => Ok(s),
    None => Err(AppError::RecordNotFound(format!(
      "Failed to get total collab bytes for workspace_id: {}",
      workspace_id
    ))),
  }
}

#[inline]
pub async fn select_workspace_name_from_workspace_id(
  pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<Option<String>, AppError> {
  let workspace_name = sqlx::query_scalar!(
    r#"
      SELECT workspace_name
      FROM public.af_workspace
      WHERE workspace_id = $1
    "#,
    workspace_id
  )
  .fetch_one(pool)
  .await?;
  Ok(workspace_name)
}

#[inline]
pub async fn select_workspace_member_count_from_workspace_id(
  pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<Option<i64>, AppError> {
  let workspace_count = sqlx::query_scalar!(
    r#"
      SELECT COUNT(*)
      FROM public.af_workspace_member
      WHERE workspace_id = $1
      AND role_id != $2
    "#,
    workspace_id,
    AFRole::Guest as i32, // Exclude guests from member count
  )
  .fetch_one(pool)
  .await?;
  Ok(workspace_count)
}

#[inline]
pub async fn select_workspace_pending_invitations(
  pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<HashMap<String, Uuid>, AppError> {
  let res = sqlx::query!(
    r#"
      SELECT id, invitee_email
      FROM public.af_workspace_invitation
      WHERE workspace_id = $1
      AND status = 0
    "#,
    workspace_id
  )
  .fetch_all(pool)
  .await?;

  let inv_id_by_email = res
    .into_iter()
    .map(|row| (row.invitee_email, row.id))
    .collect::<HashMap<String, Uuid>>();

  Ok(inv_id_by_email)
}

#[inline]
pub async fn is_workspace_exist<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
) -> Result<bool, AppError> {
  let exists = sqlx::query_scalar!(
    r#"
    SELECT EXISTS(
      SELECT 1
      FROM af_workspace
      WHERE workspace_id = $1
    ) AS user_exists;
  "#,
    workspace_id
  )
  .fetch_one(executor)
  .await?;

  Ok(exists.unwrap_or(false))
}

pub async fn select_workspace_settings<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
) -> Result<Option<AFWorkspaceSettings>, AppError> {
  let json = sqlx::query_scalar!(
    r#"SElECT settings FROM af_workspace WHERE workspace_id = $1"#,
    workspace_id
  )
  .fetch_one(executor)
  .await?;

  match json {
    None => Ok(None),
    Some(value) => {
      let settings: AFWorkspaceSettings = serde_json::from_value(value)?;
      Ok(Some(settings))
    },
  }
}
pub async fn upsert_workspace_settings(
  tx: &mut Transaction<'_, Postgres>,
  workspace_id: &Uuid,
  settings: &AFWorkspaceSettings,
) -> Result<(), AppError> {
  let json = serde_json::to_value(settings)?;
  sqlx::query!(
    r#"
      UPDATE af_workspace
      SET settings = $1
      WHERE workspace_id = $2
    "#,
    json,
    workspace_id
  )
  .execute(tx.deref_mut())
  .await?;

  if settings.disable_search_indexing {
    sqlx::query!(
      r#"
        DELETE FROM af_collab_embeddings e
        USING af_collab c
        WHERE e.oid = c.oid
          AND c.workspace_id = $1
      "#,
      workspace_id
    )
    .execute(tx.deref_mut())
    .await?;
  }

  Ok(())
}

pub async fn select_owner_of_published_collab<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  view_id: &Uuid,
) -> Result<Uuid, AppError> {
  let res = sqlx::query!(
    r#"
      SELECT af.uuid
      FROM af_published_collab apc
      JOIN af_user af ON af.uid = apc.published_by
      WHERE view_id = $1
    "#,
    view_id,
  )
  .fetch_one(executor)
  .await?;

  Ok(res.uuid)
}

pub async fn select_comments_for_published_view_ordered_by_recency<
  'a,
  E: Executor<'a, Database = Postgres>,
>(
  executor: E,
  view_id: &Uuid,
  user_uuid: &Option<Uuid>,
  page_owner_uuid: &Uuid,
) -> Result<Vec<GlobalComment>, AppError> {
  let user_uuid = user_uuid.unwrap_or(Uuid::nil());
  let is_page_owner = user_uuid == *page_owner_uuid;
  let comment_rows = sqlx::query_as!(
    AFGlobalCommentRow,
    r#"
      SELECT
        avc.comment_id,
        avc.created_at,
        avc.updated_at AS last_updated_at,
        avc.content,
        avc.reply_comment_id,
        avc.is_deleted,
        (au.uuid, au.name, au.email, au.metadata ->> 'icon_url') AS "user: AFWebUserWithEmailColumn",
        (NOT avc.is_deleted AND ($2 OR au.uuid = $3)) AS "can_be_deleted!"
      FROM af_published_view_comment avc
      LEFT OUTER JOIN af_user au ON avc.created_by = au.uid
      WHERE view_id = $1
      ORDER BY avc.created_at DESC
    "#,
    view_id,
    is_page_owner,
    user_uuid,
  )
  .fetch_all(executor)
  .await?;
  let comments = comment_rows.into_iter().map(|row| row.into()).collect();
  Ok(comments)
}

pub async fn insert_comment_to_published_view<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  view_id: &Uuid,
  user_uuid: &Uuid,
  content: &str,
  reply_comment_id: &Option<Uuid>,
) -> Result<(), AppError> {
  let res = sqlx::query!(
    r#"
      INSERT INTO af_published_view_comment (view_id, created_by, content, reply_comment_id)
      VALUES ($1, (SELECT uid FROM af_user WHERE uuid = $2), $3, $4)
    "#,
    view_id,
    user_uuid,
    content,
    reply_comment_id.clone(),
  )
  .execute(executor)
  .await?;

  if res.rows_affected() != 1 {
    tracing::error!(
      "Failed to insert comment to published view, view_id: {}, user_id: {}, content: {}, rows_affected: {}",
      view_id, user_uuid, content, res.rows_affected()
    );
  }

  Ok(())
}

pub async fn update_comment_deletion_status<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  comment_id: &Uuid,
) -> Result<(), AppError> {
  let res = sqlx::query!(
    r#"
      UPDATE af_published_view_comment
      SET is_deleted = true
      WHERE comment_id = $1
    "#,
    comment_id,
  )
  .execute(executor)
  .await?;

  if res.rows_affected() != 1 {
    tracing::error!(
      "Failed to update deletion status for comment, comment_id: {}, rows_affected: {}",
      comment_id,
      res.rows_affected()
    );
  }

  Ok(())
}

pub async fn select_reactions_for_published_view_ordered_by_reaction_type_creation_time<
  'a,
  E: Executor<'a, Database = Postgres>,
>(
  executor: E,
  view_id: &Uuid,
) -> Result<Vec<Reaction>, AppError> {
  let reaction_rows = sqlx::query_as!(
    AFReactionRow,
    r#"
      SELECT
        avr.comment_id,
        avr.reaction_type,
        ARRAY_AGG((au.uuid, au.name, au.email, au.metadata ->> 'icon_url')) AS "react_users!: Vec<AFWebUserWithEmailColumn>"
      FROM af_published_view_reaction avr
      INNER JOIN af_user au ON avr.created_by = au.uid
      WHERE view_id = $1
      GROUP BY comment_id, reaction_type
      ORDER BY MIN(avr.created_at)
    "#,
    view_id,
  )
  .fetch_all(executor)
  .await?;

  let reactions = reaction_rows.into_iter().map(|x| x.into()).collect();
  Ok(reactions)
}

pub async fn select_reactions_for_comment_ordered_by_reaction_type_creation_time<
  'a,
  E: Executor<'a, Database = Postgres>,
>(
  executor: E,
  comment_id: &Uuid,
) -> Result<Vec<Reaction>, AppError> {
  let reaction_rows = sqlx::query_as!(
    AFReactionRow,
    r#"
      SELECT
        avr.reaction_type,
        ARRAY_AGG((au.uuid, au.name, au.email, au.metadata ->> 'icon_url')) AS "react_users!: Vec<AFWebUserWithEmailColumn>",
        avr.comment_id
      FROM af_published_view_reaction avr
      INNER JOIN af_user au ON avr.created_by = au.uid
      WHERE comment_id = $1
      GROUP BY comment_id, reaction_type
      ORDER BY MIN(avr.created_at)
    "#,
    comment_id,
  )
  .fetch_all(executor)
  .await?;

  let reactions = reaction_rows.into_iter().map(|x| x.into()).collect();
  Ok(reactions)
}

pub async fn insert_reaction_on_comment<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  comment_id: &Uuid,
  view_id: &Uuid,
  user_uuid: &Uuid,
  reaction_type: &str,
) -> Result<(), AppError> {
  let res = sqlx::query!(
    r#"
      INSERT INTO af_published_view_reaction (comment_id, view_id, created_by, reaction_type)
      VALUES ($1, $2, (SELECT uid FROM af_user WHERE uuid = $3), $4)
    "#,
    comment_id,
    view_id,
    user_uuid,
    reaction_type,
  )
  .execute(executor)
  .await?;

  if res.rows_affected() != 1 {
    tracing::error!(
      "Failed to insert reaction to comment, comment_id: {}, user_id: {}, reaction_type: {}, rows_affected: {}",
      comment_id, user_uuid, reaction_type, res.rows_affected()
    );
  };

  Ok(())
}

pub async fn delete_reaction_from_comment<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  comment_id: &Uuid,
  user_uuid: &Uuid,
  reaction_type: &str,
) -> Result<(), AppError> {
  let res = sqlx::query!(
    r#"
      DELETE FROM af_published_view_reaction
      WHERE comment_id = $1 AND created_by = (SELECT uid FROM af_user WHERE uuid = $2) AND reaction_type = $3
    "#,
    comment_id,
    user_uuid,
    reaction_type,
  ).execute(executor).await?;

  if res.rows_affected() != 1 {
    tracing::error!(
      "Failed to delete reaction from published comment, comment_id: {}, user_id: {}, reaction_type: {}, rows_affected: {}",
      comment_id, user_uuid, reaction_type, res.rows_affected()
    );
  };

  Ok(())
}

pub async fn select_user_is_invitee_for_workspace_invitation(
  pg_pool: &PgPool,
  invitee_uuid: &Uuid,
  invite_id: &Uuid,
) -> Result<bool, AppError> {
  let res = sqlx::query_scalar!(
    r#"
      SELECT EXISTS(
        SELECT 1
        FROM af_workspace_invitation
        WHERE id = $1 AND LOWER(invitee_email) = (SELECT LOWER(email) FROM af_user WHERE uuid = $2)
      )
    "#,
    invite_id,
    invitee_uuid,
  )
  .fetch_one(pg_pool)
  .await?;
  res.map_or(Ok(false), Ok)
}

pub async fn select_import_task(
  pg_pool: &PgPool,
  task_id: &Uuid,
) -> Result<AFImportTask, AppError> {
  let query = String::from("SELECT * FROM af_import_task WHERE task_id = $1");
  let import_task = sqlx::query_as::<_, AFImportTask>(&query)
    .bind(task_id)
    .fetch_one(pg_pool)
    .await?;
  Ok(import_task)
}

/// Get the import task for the user
/// Status of the file import (e.g., 0 for pending, 1 for completed, 2 for failed)
pub async fn select_import_task_by_state(
  user_id: i64,
  pg_pool: &PgPool,
  filter_by_status: Option<ImportTaskState>,
) -> Result<Vec<AFImportTask>, AppError> {
  let mut query = String::from("SELECT * FROM af_import_task WHERE created_by = $1");
  if filter_by_status.is_some() {
    query.push_str(" AND status = $2");
  }
  query.push_str(" ORDER BY created_at DESC");

  let import_tasks = if let Some(status) = filter_by_status {
    sqlx::query_as::<_, AFImportTask>(&query)
      .bind(user_id)
      .bind(status as i32)
      .fetch_all(pg_pool)
      .await?
  } else {
    sqlx::query_as::<_, AFImportTask>(&query)
      .bind(user_id)
      .fetch_all(pg_pool)
      .await?
  };

  Ok(import_tasks)
}

#[derive(Clone, Debug)]
pub enum ImportTaskState {
  Pending = 0,
  Completed = 1,
  Failed = 2,
  Expire = 3,
  Cancel = 4,
}

impl From<i16> for ImportTaskState {
  fn from(val: i16) -> Self {
    match val {
      0 => ImportTaskState::Pending,
      1 => ImportTaskState::Completed,
      2 => ImportTaskState::Failed,
      4 => ImportTaskState::Cancel,
      _ => ImportTaskState::Pending,
    }
  }
}

/// Update import task status
///  0 => Pending,
///   1 => Completed,
///   2 => Failed,
///   3 => Expire,
pub async fn update_import_task_status<'a, E: Executor<'a, Database = Postgres>>(
  task_id: &Uuid,
  new_status: ImportTaskState,
  executor: E,
) -> Result<(), AppError> {
  let query = "UPDATE af_import_task SET status = $1 WHERE task_id = $2";
  sqlx::query(query)
    .bind(new_status as i16)
    .bind(task_id)
    .execute(executor)
    .await
    .map_err(|err| {
      AppError::Internal(anyhow::anyhow!(
        "Failed to update status for task_id {}: {:?}",
        task_id,
        err
      ))
    })?;

  Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_import_task(
  uid: i64,
  task_id: Uuid,
  file_size: i64,
  workspace_id: String,
  created_by: i64,
  metadata: Option<serde_json::Value>,
  presigned_url: Option<String>,
  pg_pool: &PgPool,
) -> Result<(), AppError> {
  let query = r#"
        INSERT INTO af_import_task (task_id, file_size, workspace_id, created_by, status, metadata, uid, file_url)
        VALUES ($1, $2, $3, $4, $5, COALESCE($6, '{}'), $7, $8)
    "#;

  sqlx::query(query)
    .bind(task_id)
    .bind(file_size)
    .bind(workspace_id)
    .bind(created_by)
    .bind(ImportTaskState::Pending as i32)
    .bind(metadata)
    .bind(uid)
    .bind(presigned_url)
    .execute(pg_pool)
    .await
    .map_err(|err| {
      AppError::Internal(anyhow::anyhow!(
        "Failed to create a new import task: {:?}",
        err
      ))
    })?;

  Ok(())
}

pub async fn update_import_task_metadata(
  task_id: Uuid,
  new_metadata: serde_json::Value,
  pg_pool: &PgPool,
) -> Result<(), AppError> {
  let query = r#"
        UPDATE af_import_task
        SET metadata = metadata || $1
        WHERE task_id = $2
    "#;

  sqlx::query(query)
    .bind(new_metadata)
    .bind(task_id)
    .execute(pg_pool)
    .await
    .map_err(|err| {
      AppError::Internal(anyhow::anyhow!(
        "Failed to update metadata for task_id {}: {:?}",
        task_id,
        err
      ))
    })?;

  Ok(())
}

#[inline]
pub async fn select_publish_name_exists(
  pg_pool: &PgPool,
  workspace_uuid: &Uuid,
  publish_name: &str,
) -> Result<bool, AppError> {
  let exists = sqlx::query_scalar!(
    r#"
      SELECT EXISTS(
        SELECT 1
        FROM af_published_collab
        WHERE workspace_id = $1
          AND publish_name = $2
          AND unpublished_at IS NULL
      )
    "#,
    workspace_uuid,
    publish_name
  )
  .fetch_one(pg_pool)
  .await?;

  Ok(exists.unwrap_or(false))
}

#[inline]
pub async fn select_view_id_from_publish_name(
  pg_pool: &PgPool,
  workspace_uuid: &Uuid,
  publish_name: &str,
) -> Result<Option<Uuid>, AppError> {
  let res = sqlx::query_scalar!(
    r#"
      SELECT view_id
      FROM af_published_collab
      WHERE workspace_id = $1
        AND unpublished_at IS NULL
        AND publish_name = $2
    "#,
    workspace_uuid,
    publish_name
  )
  .fetch_optional(pg_pool)
  .await?;

  Ok(res)
}

pub async fn select_invited_workspace_id(
  pg_pool: &PgPool,
  invitation_code: &str,
) -> Result<Uuid, AppError> {
  let res = sqlx::query_scalar!(
    r#"
      SELECT workspace_id
      FROM af_workspace_invite_code
      WHERE invite_code = $1
        AND (expires_at IS NULL OR expires_at > NOW())
    "#,
    invitation_code
  )
  .fetch_one(pg_pool)
  .await?;

  Ok(res)
}

pub async fn select_invitation_code_info<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  invite_code: &str,
  uid: i64,
) -> Result<Vec<InvitationCodeInfo>, AppError> {
  let info_list = sqlx::query_as!(
    InvitationCodeInfo,
    r#"
      WITH invited_workspace_member AS (
        SELECT
          invite_code,
          COUNT(*) AS member_count,
          COUNT(CASE WHEN uid = $2 THEN uid END) > 0 AS is_member
        FROM af_workspace_invite_code
        JOIN af_workspace_member USING (workspace_id)
        WHERE invite_code = $1
        AND (expires_at IS NULL OR expires_at > NOW())
        GROUP BY invite_code
      )
      SELECT
      workspace_id,
      owner_profile.name AS "owner_name!",
      owner_profile.metadata ->> 'icon_url' AS owner_avatar,
      af_workspace.workspace_name AS "workspace_name!",
      af_workspace.icon AS workspace_icon_url,
      invited_workspace_member.member_count AS "member_count!",
      invited_workspace_member.is_member AS "is_member!"
      FROM af_workspace_invite_code
      JOIN af_workspace USING (workspace_id)
      JOIN af_user AS owner_profile ON af_workspace.owner_uid = owner_profile.uid
      JOIN invited_workspace_member USING (invite_code)
      WHERE invite_code = $1
    "#,
    invite_code,
    uid
  )
  .fetch_all(executor)
  .await?;

  Ok(info_list)
}

pub async fn upsert_workspace_member_uid<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
  uid: i64,
  role: AFRole,
) -> Result<(), AppError> {
  let role_id = role as i32;
  sqlx::query!(
    r#"
      INSERT INTO af_workspace_member (workspace_id, uid, role_id)
      VALUES ($1, $2, $3)
      ON CONFLICT (workspace_id, uid) DO NOTHING
    "#,
    workspace_id,
    uid,
    role_id,
  )
  .execute(executor)
  .await?;

  Ok(())
}

pub async fn select_invite_code_for_workspace_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
) -> Result<Option<String>, AppError> {
  let code = sqlx::query_scalar!(
    r#"
      SELECT invite_code
      FROM af_workspace_invite_code
      WHERE workspace_id = $1
    "#,
    workspace_id,
  )
  .fetch_optional(executor)
  .await?;

  Ok(code)
}

pub async fn delete_all_invite_code_for_workspace<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
      DELETE FROM af_workspace_invite_code
      WHERE workspace_id = $1
    "#,
    workspace_id,
  )
  .execute(executor)
  .await?;

  Ok(())
}

pub async fn insert_workspace_invite_code<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
  code: &str,
  expires_at: Option<&chrono::DateTime<Utc>>,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
      INSERT INTO af_workspace_invite_code (workspace_id, invite_code, expires_at)
      VALUES ($1, $2, $3)
    "#,
    workspace_id,
    code,
    expires_at.map(|dt| dt.naive_utc()),
  )
  .execute(executor)
  .await?;

  Ok(())
}

#[inline]
pub async fn select_workspace_member_uids<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
) -> Result<Vec<i64>, AppError> {
  let member_uids = sqlx::query_scalar!(
    r#"
      SELECT uid
      FROM af_workspace_member
      WHERE workspace_id = $1
      ORDER BY created_at ASC
    "#,
    workspace_id,
  )
  .fetch_all(executor)
  .await?;

  Ok(member_uids)
}

pub async fn select_workspace_mentionable_members_or_guests<
  'a,
  E: Executor<'a, Database = Postgres>,
>(
  executor: E,
  workspace_id: &Uuid,
) -> Result<Vec<MentionableWorkspaceMemberOrGuest>, AppError> {
  let members = sqlx::query_as!(
    MentionableWorkspaceMemberOrGuest,
    r#"
      SELECT
        au.uuid,
        COALESCE(awmp.name, au.name) AS "name!",
        au.email,
        awm.role_id AS "role!",
        COALESCE(awmp.avatar_url, au.metadata ->> 'icon_url') AS "avatar_url",
        awmp.cover_image_url,
        awmp.description
      FROM af_workspace_member awm
      JOIN af_user au ON awm.uid = au.uid
      LEFT JOIN af_workspace_member_profile awmp ON (awm.uid = awmp.uid AND awm.workspace_id = awmp.workspace_id)
      WHERE awm.workspace_id = $1
    "#,
    workspace_id,
  )
  .fetch_all(executor)
  .await?;

  Ok(members)
}

pub async fn select_workspace_mentionable_member_or_guest_by_uuid<
  'a,
  E: Executor<'a, Database = Postgres>,
>(
  executor: E,
  workspace_id: &Uuid,
  user_id: &Uuid,
) -> Result<Option<MentionableWorkspaceMemberOrGuest>, AppError> {
  let member = sqlx::query_as!(
    MentionableWorkspaceMemberOrGuest,
    r#"
      SELECT
        au.uuid,
        COALESCE(awmp.name, au.name) AS "name!",
        au.email,
        awm.role_id AS "role!",
        COALESCE(awmp.avatar_url, au.metadata ->> 'icon_url') AS "avatar_url",
        awmp.cover_image_url,
        awmp.description
      FROM af_workspace_member awm
      JOIN af_user au ON awm.uid = au.uid
      LEFT JOIN af_workspace_member_profile awmp ON (awm.uid = awmp.uid AND awm.workspace_id = awmp.workspace_id)
      WHERE awm.workspace_id = $1
      AND au.uuid = $2
    "#,
    workspace_id,
    user_id,
  )
  .fetch_optional(executor)
  .await?;

  Ok(member)
}

pub async fn upsert_workspace_member_profile<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: &Uuid,
  uid: i64,
  updated_profile: &WorkspaceMemberProfile,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
      INSERT INTO af_workspace_member_profile (workspace_id, uid, name, avatar_url, cover_image_url, description)
      VALUES ($1, $2, $3, $4, $5, $6)
      ON CONFLICT (workspace_id, uid) DO UPDATE
      SET name = EXCLUDED.name,
          avatar_url = EXCLUDED.avatar_url,
          cover_image_url = EXCLUDED.cover_image_url,
          description = EXCLUDED.description
    "#,
    workspace_id,
    uid,
    updated_profile.name,
    updated_profile.avatar_url,
    updated_profile.cover_image_url,
    updated_profile.description
  )
  .execute(executor)
  .await?;

  Ok(())
}
