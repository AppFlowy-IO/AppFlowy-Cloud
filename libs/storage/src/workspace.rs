use sqlx::{types::uuid, PgPool};

use crate::entities::AfWorkspace;

pub async fn create_workspace_if_not_exists(
  pool: &PgPool,
  owner_uid: uuid::Uuid,
) -> Result<(), sqlx::Error> {
  sqlx::query!(
    r#"
        INSERT INTO af_workspace (owner_uid)
        SELECT $1
        WHERE NOT EXISTS (
            SELECT 1 FROM public.af_workspace WHERE owner_uid = $1
        )
        "#,
    owner_uid
  )
  .execute(pool)
  .await?;
  Ok(())
}

pub async fn select_all_workspaces_owned(
  pool: &PgPool,
  owner_uid: uuid::Uuid,
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
