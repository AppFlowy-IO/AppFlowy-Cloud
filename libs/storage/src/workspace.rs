use sqlx::{
  types::{uuid, Uuid},
  PgPool,
};

use storage_entity::{AFRole, AFUserProfileView, AFWorkspace, AFWorkspaceMember};

pub async fn select_all_workspaces_owned(
  pool: &PgPool,
  owner_uuid: &Uuid,
) -> Result<Vec<AFWorkspace>, sqlx::Error> {
  sqlx::query_as!(
    AFWorkspace,
    r#"
        SELECT * FROM public.af_workspace WHERE owner_uid = (
            SELECT uid FROM public.af_user WHERE uuid = $1
            )
        "#,
    owner_uuid
  )
  .fetch_all(pool)
  .await
}

pub async fn select_user_is_workspace_owner(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_uuid: &Uuid,
) -> Result<bool, sqlx::Error> {
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

pub async fn insert_workspace_members(
  pool: &PgPool,
  workspace_id: &uuid::Uuid,
  member_emails: &[String],
  role: AFRole,
) -> Result<(), sqlx::Error> {
  sqlx::query!(
    r#"
        INSERT INTO public.af_workspace_member (workspace_id, uid, role_id)
        SELECT $1, af_user.uid, $3
        FROM unnest($2::text[]) AS emails(email)
        JOIN public.af_user ON af_user.email = emails.email
        ON CONFLICT (workspace_id, uid)
        DO NOTHING
        "#,
    workspace_id,
    member_emails,
    role.id()
  )
  .execute(pool)
  .await?;

  Ok(())
}

pub async fn delete_workspace_members(
  pool: &PgPool,
  workspace_id: &uuid::Uuid,
  member_emails: &[String],
) -> Result<(), sqlx::Error> {
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
) -> Result<Vec<AFWorkspaceMember>, sqlx::Error> {
  sqlx::query_as!(
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
  .await
}

pub async fn select_user_profile_view_by_uuid(
  pool: &PgPool,
  user_uuid: &Uuid,
) -> Result<Option<AFUserProfileView>, sqlx::Error> {
  sqlx::query_as!(
    AFUserProfileView,
    r#"
        SELECT *
        FROM public.af_user_profile_view WHERE uuid = $1
        "#,
    user_uuid
  )
  .fetch_optional(pool)
  .await
}
