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

pub async fn select_user_is_workspace_owner(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_uuid: &Uuid,
) -> Result<bool, DatabaseError> {
  let exists = sqlx::query_scalar!(
    r#"
        SELECT EXISTS(
          SELECT 1 FROM public.af_workspace
          WHERE workspace_id = $1 AND owner_uid = (
            SELECT uid FROM public.af_user WHERE uuid = $2
          )
        )
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
    role.id()
  )
  .execute(txn.deref_mut())
  .await?;

  Ok(())
}

pub async fn delete_workspace_members(
  pool: &PgPool,
  workspace_id: &uuid::Uuid,
  member_emails: &[String],
) -> Result<(), DatabaseError> {
  sqlx::query!(
    r#"
        DELETE FROM public.af_workspace_member
        WHERE workspace_id = $1 AND uid IN (
            SELECT uid FROM public.af_user WHERE email = ANY($2::text[])
        )
        "#,
    workspace_id,
    &member_emails
  )
  .execute(pool)
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
