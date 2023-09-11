use sqlx::{
  types::{uuid, Uuid},
  PgPool,
};

use crate::entities::{AfUserProfileView, AfWorkspace};

pub async fn create_user_if_not_exists(
  pool: &PgPool,
  gotrue_uuid: uuid::Uuid,
) -> Result<(), sqlx::Error> {
  sqlx::query!(
    r#"
        INSERT INTO af_user (uuid)
        SELECT $1
        WHERE NOT EXISTS (
            SELECT 1 FROM public.af_user WHERE uuid = $1
        )
        "#,
    gotrue_uuid
  )
  .execute(pool)
  .await?;
  Ok(())
}

pub async fn select_all_workspaces_owned(
  pool: &PgPool,
  owner_uid: i64,
) -> Result<Vec<AfWorkspace>, sqlx::Error> {
  sqlx::query_as!(
    AfWorkspace,
    r#"
        SELECT * FROM public.af_workspace WHERE owner_uid = $1
        "#,
    owner_uid
  )
  .fetch_all(pool)
  .await
}

pub async fn select_user_profile_view_by_uuid(
  pool: &PgPool,
  gotrue_uuid: Uuid,
) -> Result<Option<AfUserProfileView>, sqlx::Error> {
  sqlx::query_as!(
    AfUserProfileView,
    r#"
        SELECT *
        FROM public.af_user_profile_view WHERE uuid = $1
        "#,
    gotrue_uuid
  )
  .fetch_optional(pool)
  .await
}
