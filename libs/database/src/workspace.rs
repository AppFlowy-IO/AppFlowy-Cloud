use database_entity::dto::AFRole;
use sqlx::{
  types::{uuid, Uuid},
  Executor, PgPool, Postgres, Transaction,
};
use std::ops::DerefMut;
use tracing::{event, instrument};

use crate::user::select_uid_from_email;
use database_entity::error::DatabaseError;
use database_entity::pg_row::{AFUserProfileRow, AFWorkspaceMemberRow, AFWorkspaceRow};

/// Checks whether a user, identified by a UUID, is an 'Owner' of a workspace, identified by its
/// workspace_id.
#[inline]
pub async fn select_user_is_workspace_owner(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_uuid: &Uuid,
) -> Result<bool, DatabaseError> {
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

#[inline]
pub async fn select_user_role<'a, E: Executor<'a, Database = Postgres>>(
  exectuor: E,
  uid: &i64,
  workspace_uuid: &Uuid,
) -> Result<AFRole, DatabaseError> {
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

/// Checks the user's permission to edit a collab object.
/// user can edit collab if:
/// 1. user is the member of the workspace
/// 2. the collab object is not exist
/// 3. the collab object is exist and the user is the member of the collab and the role is owner or member
#[allow(dead_code)]
pub async fn select_user_can_edit_collab(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
  object_id: &str,
) -> Result<bool, DatabaseError> {
  let permission_check = sqlx::query_scalar!(
    r#"
    WITH workspace_check AS (
        SELECT EXISTS(
            SELECT 1
            FROM af_workspace_member
            WHERE af_workspace_member.uid = (SELECT uid FROM af_user WHERE uuid = $1) AND
            af_workspace_member.workspace_id = $3
        ) AS "workspace_exists"
    ),
    collab_check AS (
        SELECT EXISTS(
            SELECT 1
            FROM af_collab_member
            WHERE oid = $2
        ) AS "collab_exists"
    )
    SELECT 
        NOT collab_check.collab_exists OR (
            workspace_check.workspace_exists AND 
            EXISTS(
                SELECT 1
                FROM af_collab_member
                JOIN af_permissions ON af_collab_member.permission_id = af_permissions.id
                WHERE 
                    af_collab_member.uid = (SELECT uid FROM af_user WHERE uuid = $1) AND 
                    af_collab_member.oid = $2 AND 
                    af_permissions.access_level > 20
            )
        ) AS "permission_check"
    FROM workspace_check, collab_check;
     "#,
    user_uuid,
    object_id,
    workspace_id,
  )
  .fetch_one(pg_pool)
  .await?;

  Ok(permission_check.unwrap_or(false))
}

#[inline]
pub async fn insert_workspace_member_with_txn(
  txn: &mut Transaction<'_, sqlx::Postgres>,
  workspace_id: &uuid::Uuid,
  member_email: &str,
  role: AFRole,
) -> Result<(), DatabaseError> {
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
#[instrument(level = "trace", skip(pool, email, role), err)]
pub async fn upsert_workspace_member(
  pool: &PgPool,
  workspace_id: &Uuid,
  email: &str,
  role: Option<AFRole>,
) -> Result<(), sqlx::Error> {
  if role.is_none() {
    return Ok(());
  }

  event!(
    tracing::Level::TRACE,
    "update workspace member: workspace_id:{}, uid {:?}, role:{:?}",
    workspace_id,
    select_uid_from_email(pool, email).await,
    role
  );

  let role_id: i32 = role.unwrap().into();
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
  _user_uuid: &Uuid,
  txn: &mut Transaction<'_, sqlx::Postgres>,
  workspace_id: &Uuid,
  member_email: &str,
) -> Result<(), DatabaseError> {
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
    return Err(DatabaseError::NotEnoughPermissions(
      "Owner cannot be deleted".to_string(),
    ));
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

/// returns a list of workspace members, sorted by their creation time.
#[inline]
pub async fn select_workspace_member_list(
  pg_pool: &PgPool,
  workspace_id: &uuid::Uuid,
) -> Result<Vec<AFWorkspaceMemberRow>, DatabaseError> {
  let members = sqlx::query_as!(
    AFWorkspaceMemberRow,
    r#"
    SELECT af_user.email,
    af_workspace_member.role_id AS role
    FROM public.af_workspace_member
        JOIN public.af_user ON af_workspace_member.uid = af_user.uid
    WHERE af_workspace_member.workspace_id = $1
    ORDER BY af_workspace_member.created_at ASC;
    "#,
    workspace_id
  )
  .fetch_all(pg_pool)
  .await?;
  Ok(members)
}

#[inline]
pub async fn select_workspace_member(
  pg_pool: &PgPool,
  uid: &i64,
  workspace_id: &Uuid,
) -> Result<AFWorkspaceMemberRow, DatabaseError> {
  let member = sqlx::query_as!(
    AFWorkspaceMemberRow,
    r#"
    SELECT af_user.email, af_workspace_member.role_id AS role
    FROM public.af_workspace_member
      JOIN public.af_user ON af_workspace_member.uid = af_user.uid
    WHERE af_workspace_member.workspace_id = $1 
    AND af_workspace_member.uid = $2 
    "#,
    workspace_id,
    uid,
  )
  .fetch_one(pg_pool)
  .await?;
  Ok(member)
}

#[inline]
pub async fn select_user_profile<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
) -> Result<Option<AFUserProfileRow>, DatabaseError> {
  let user_profile = sqlx::query_as!(
    AFUserProfileRow,
    r#"
      SELECT *
      FROM public.af_user_profile_view WHERE uuid = $1
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
) -> Result<AFWorkspaceRow, DatabaseError> {
  let workspace = sqlx::query_as!(
    AFWorkspaceRow,
    r#"
       SELECT * FROM public.af_workspace WHERE workspace_id = $1
    "#,
    workspace_id
  )
  .fetch_one(executor)
  .await?;
  Ok(workspace)
}
#[inline]
pub async fn update_updated_at_of_workspace<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
) -> Result<(), DatabaseError> {
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

/// Returns a list of workspaces that the user is a member of.
#[inline]
pub async fn select_user_workspace<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
) -> Result<Vec<AFWorkspaceRow>, DatabaseError> {
  let workspaces = sqlx::query_as!(
    AFWorkspaceRow,
    r#"
      SELECT w.* 
      FROM af_workspace w
      JOIN af_workspace_member wm ON w.workspace_id = wm.workspace_id
      WHERE wm.uid = (
         SELECT uid FROM public.af_user WHERE uuid = $1
      );
    "#,
    user_uuid
  )
  .fetch_all(executor)
  .await?;
  Ok(workspaces)
}

#[inline]
pub async fn select_all_user_workspaces(
  pool: &PgPool,
  owner_uuid: &Uuid,
) -> Result<Vec<AFWorkspaceRow>, DatabaseError> {
  let workspaces = sqlx::query_as!(
    AFWorkspaceRow,
    r#"
      SELECT * FROM public.af_workspace WHERE owner_uid = (
        SELECT uid FROM public.af_user WHERE uuid = $1
      )
    "#,
    owner_uuid
  )
  .fetch_all(pool)
  .await?;
  Ok(workspaces)
}
