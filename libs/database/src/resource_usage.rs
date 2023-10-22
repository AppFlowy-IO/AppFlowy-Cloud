use database_entity::database_error::DatabaseError;
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
) -> Result<bool, DatabaseError> {
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
) -> Result<AFBlobMetadata, DatabaseError> {
  let metadata = sqlx::query_as!(
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
  .await?;
  Ok(metadata)
}

#[instrument(level = "trace", skip_all, err)]
pub async fn delete_blob_metadata(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  file_id: &str,
) -> Result<AFBlobMetadata, DatabaseError> {
  let metadata = sqlx::query_as!(
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
  .await?;
  Ok(metadata)
}

#[instrument(level = "trace", skip_all, err)]
pub async fn get_blob_metadata(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  file_id: &str,
) -> Result<AFBlobMetadata, DatabaseError> {
  let metadata = sqlx::query_as!(
    AFBlobMetadata,
    r#"
        SELECT * FROM af_blob_metadata
        WHERE workspace_id = $1 AND file_id = $2
        "#,
    workspace_id,
    file_id,
  )
  .fetch_one(pg_pool)
  .await?;
  Ok(metadata)
}

/// Return all blob metadata of a workspace
#[instrument(level = "trace", skip_all, err)]
pub async fn get_all_workspace_blob_metadata(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<Vec<AFBlobMetadata>, DatabaseError> {
  let all_metadata = sqlx::query_as!(
    AFBlobMetadata,
    r#"
        SELECT * FROM af_blob_metadata
        WHERE workspace_id = $1 
        "#,
    workspace_id,
  )
  .fetch_all(pg_pool)
  .await?;
  Ok(all_metadata)
}

/// Return all blob ids of a workspace
#[instrument(level = "trace", skip_all, err)]
pub async fn get_all_workspace_blob_ids(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<Vec<String>, DatabaseError> {
  let file_ids = sqlx::query!(
    r#"
    SELECT file_id FROM af_blob_metadata
    WHERE workspace_id = $1
    "#,
    workspace_id
  )
  .fetch_all(pg_pool)
  .await?
  .into_iter()
  .map(|record| record.file_id)
  .collect();
  Ok(file_ids)
}

/// Return the total size of a workspace in bytes
#[instrument(level = "trace", skip_all, err)]
pub async fn get_workspace_usage_size(
  pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<u64, DatabaseError> {
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
