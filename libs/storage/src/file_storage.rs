use sqlx::{PgPool, Transaction};
use storage_entity::AFFileMetadata;

pub async fn insert_file_metadata(
  trans: &mut Transaction<'_, sqlx::Postgres>,
  user: &uuid::Uuid,
  path: &str,
  file_type: &str,
  file_size: i64,
) -> Result<AFFileMetadata, sqlx::Error> {
  sqlx::query_as!(
    AFFileMetadata,
    r#"
        INSERT INTO af_file_metadata (owner_uid, path, file_type, file_size)
        SELECT uid, $2, $3, $4
        FROM af_user
        WHERE uuid = $1
        ON CONFLICT (owner_uid, path) DO UPDATE SET
            file_type = $3,
            file_size = $4
        RETURNING *
        "#,
    user,
    path,
    file_type,
    file_size
  )
  .fetch_one(trans.as_mut())
  .await
}

pub async fn delete_file_metadata(
  trans: &mut Transaction<'_, sqlx::Postgres>,
  user: &uuid::Uuid,
  path: &str,
) -> Result<AFFileMetadata, sqlx::Error> {
  sqlx::query_as!(
    AFFileMetadata,
    r#"
        DELETE FROM af_file_metadata
        WHERE owner_uid = (
            SELECT uid
            FROM af_user
            WHERE uuid = $1
        ) AND path = $2
        RETURNING *
        "#,
    user,
    path,
  )
  .fetch_one(trans.as_mut())
  .await
}

pub async fn get_file_metadata(
  pg_pool: &PgPool,
  user: &uuid::Uuid,
  path: &str,
) -> Result<AFFileMetadata, sqlx::Error> {
  sqlx::query_as!(
    AFFileMetadata,
    r#"
        SELECT * FROM af_file_metadata
        WHERE owner_uid = (
            SELECT uid
            FROM af_user
            WHERE uuid = $1
        ) AND path = $2
        "#,
    user,
    path,
  )
  .fetch_one(pg_pool)
  .await
}
