use sqlx::{
  types::{uuid, Uuid},
  PgPool, Transaction,
};
use std::ops::DerefMut;

use database_entity::database_error::DatabaseError;
use database_entity::{AFRole, AFUserProfileView, AFWorkspace, AFWorkspaceMember};

pub async fn select_all_workspaces_owned(
  pool: &PgPool,
  owner_uuid: &Uuid,
) -> Result<Vec<AFWorkspace>, DatabaseError> {
  let workspaces = sqlx::query_as!(
    AFWorkspace,
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

/// Checks whether a user, identified by a UUID, is an 'Owner' of a workspace, identified by its
/// workspace_id.
pub async fn select_user_is_workspace_owner(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_uuid: &Uuid,
) -> Result<bool, DatabaseError> {
  // 1. Identifies the user's UID in the 'af_user' table using the provided user UUID ($2).
  // 2. Then, it checks the 'af_workspace_member' table to find a record that matches the provided workspace_id ($1) and the identified UID.
  // 3. It joins with 'af_roles' to ensure that the role associated with the workspace member is 'Owner'.
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

pub async fn insert_workspace_member(
  txn: &mut Transaction<'_, sqlx::Postgres>,
  workspace_id: &uuid::Uuid,
  member_email: String,
  role: AFRole,
) -> Result<(), DatabaseError> {
  let role_id: i32 = role.into();
  sqlx::query!(
    r#"
      INSERT INTO public.af_workspace_member (workspace_id, uid, role_id)
      SELECT $1, af_user.uid, $3
      FROM public.af_user 
      WHERE af_user.email = $2 
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

pub async fn upsert_workspace_member(
  pool: &PgPool,
  workspace_id: &Uuid,
  email: &str,
  role: Option<AFRole>,
) -> Result<(), sqlx::Error> {
  if role.is_none() {
    return Ok(());
  }

  let role_id: Option<i32> = role.map(|role| role.into());
  sqlx::query!(
    r#"
        UPDATE af_workspace_member
        SET 
            role_id = COALESCE($1, role_id)
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

pub async fn delete_workspace_members(
  _user_uuid: &Uuid,
  txn: &mut Transaction<'_, sqlx::Postgres>,
  workspace_id: &Uuid,
  member_email: String,
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
    -- Ensure the user to be deleted is not the original owner
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

pub async fn select_workspace_members(
  pg_pool: &PgPool,
  workspace_id: &uuid::Uuid,
) -> Result<Vec<AFWorkspaceMember>, DatabaseError> {
  let members = sqlx::query_as!(
    AFWorkspaceMember,
    r#"
        SELECT af_user.email, af_workspace_member.role_id AS role
        FROM public.af_workspace_member
        JOIN public.af_user ON af_workspace_member.uid = af_user.uid
        WHERE af_workspace_member.workspace_id = $1
        "#,
    workspace_id
  )
  .fetch_all(pg_pool)
  .await?;
  Ok(members)
}

pub async fn select_user_profile_view_by_uuid(
  pool: &PgPool,
  user_uuid: &Uuid,
) -> Result<Option<AFUserProfileView>, DatabaseError> {
  let user_profile = sqlx::query_as!(
    AFUserProfileView,
    r#"
        SELECT *
        FROM public.af_user_profile_view WHERE uuid = $1
        "#,
    user_uuid
  )
  .fetch_optional(pool)
  .await?;
  Ok(user_profile)
}
