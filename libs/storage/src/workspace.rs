use sqlx::{
  types::{uuid, Uuid},
  PgPool,
};

use storage_entity::{AFUserProfileView, AFWorkspace};

pub async fn create_user_if_not_exists(
  pool: &PgPool,
  gotrue_uuid: &uuid::Uuid,
  email: &str,
) -> Result<(), sqlx::Error> {
  sqlx::query!(
    r#"
        INSERT INTO af_user (uuid, email)
        SELECT $1, $2
        WHERE NOT EXISTS (
            SELECT 1 FROM public.af_user WHERE email = $2
        )
        AND NOT EXISTS (
            SELECT 1 FROM public.af_user WHERE uuid = $1
        )
        "#,
    gotrue_uuid,
    email
  )
  .execute(pool)
  .await?;
  Ok(())
}

pub async fn get_user_id(pool: &PgPool, gotrue_uuid: &uuid::Uuid) -> Result<i64, sqlx::Error> {
  let uid = sqlx::query!(
    r#"
      SELECT uid FROM af_user WHERE uuid = $1
    "#,
    gotrue_uuid
  )
  .fetch_one(pool)
  .await?
  .uid;
  Ok(uid)
}

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
