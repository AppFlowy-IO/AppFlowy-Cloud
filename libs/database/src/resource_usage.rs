use database_entity::AFBlobMetadata;
use rust_decimal::prelude::ToPrimitive;
use sqlx::types::Decimal;
use sqlx::PgPool;
use tracing::instrument;
use uuid::Uuid;

#[instrument(level = "trace", skip_all, err)]
pub async fn is_blob_metadata_exists(
  pool: &PgPool,
  workspace_id: &Uuid,
  file_id: &str,
) -> Result<bool, sqlx::Error> {
  let exists: (bool,) = sqlx::query_as(
    r#"
     SELECT EXISTS (
         SELECT 1
         FROM af_blob_metadata
         WHERE workspace_id = $1 AND file_id = $2
     );
    "#,
  )
  .bind(workspace_id)
  .bind(file_id)
  .fetch_one(pool)
  .await?;

  Ok(exists.0)
}

#[instrument(level = "trace", skip_all, err)]
pub async fn insert_blob_metadata(
  pg_pool: &PgPool,
  file_id: &str,
  workspace_id: &Uuid,
  file_type: &str,
  file_size: i64,
) -> Result<AFBlobMetadata, sqlx::Error> {
  sqlx::query_as!(
    AFBlobMetadata,
    r#"
        INSERT INTO af_blob_metadata
        (workspace_id, file_id, file_type, file_size)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (workspace_id, file_id) DO UPDATE SET
            file_type = $3,
            file_size = $4
        RETURNING *
        "#,
    workspace_id,
    file_id,
    file_type,
    file_size
  )
  .fetch_one(pg_pool)
  .await
}

#[instrument(level = "trace", skip_all, err)]
pub async fn delete_blob_metadata(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  file_id: &str,
) -> Result<AFBlobMetadata, sqlx::Error> {
  sqlx::query_as!(
    AFBlobMetadata,
    r#"
        DELETE FROM af_blob_metadata
        WHERE workspace_id = $1 AND file_id = $2
        RETURNING *
        "#,
    workspace_id,
    file_id,
  )
  .fetch_one(pg_pool)
  .await
}

#[instrument(level = "trace", skip_all, err)]
pub async fn get_blob_metadata(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  file_id: &str,
) -> Result<AFBlobMetadata, sqlx::Error> {
  sqlx::query_as!(
    AFBlobMetadata,
    r#"
        SELECT * FROM af_blob_metadata
        WHERE workspace_id = $1 AND file_id = $2
        "#,
    workspace_id,
    file_id,
  )
  .fetch_one(pg_pool)
  .await
}

#[instrument(level = "trace", skip_all, err)]
pub async fn get_workspace_usage_size(
  pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<u64, sqlx::Error> {
  let row: (Option<Decimal>,) =
    sqlx::query_as(r#"SELECT SUM(file_size) FROM af_blob_metadata WHERE workspace_id = $1;"#)
      .bind(workspace_id)
      .fetch_one(pool)
      .await?;
  match row.0 {
    Some(decimal) => Ok(decimal.to_u64().unwrap_or(0)),
    None => Ok(0),
  }
}
