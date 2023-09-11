use sqlx::{types::uuid, PgPool};

pub async fn create_workspace_if_not_exists(
  pool: PgPool,
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
  .execute(&pool)
  .await?;
  Ok(())
}
