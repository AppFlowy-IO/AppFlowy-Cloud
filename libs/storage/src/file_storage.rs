use sqlx::{PgPool, Transaction};
use storage_entity::AFObjectMetadata;

pub async fn insert_object_metadata(
  _trans: &mut Transaction<'_, sqlx::Postgres>,
  _user: &uuid::Uuid,
  _path: &str,
  _file_type: &str,
  _file_size: i64,
  _s3_key: &uuid::Uuid,
) -> Result<(), sqlx::Error> {
  todo!()
}

pub async fn set_delete_object_metadata(
  _trans: &mut Transaction<'_, sqlx::Postgres>,
  _user: &uuid::Uuid,
  _path: &str,
) -> Result<String, sqlx::Error> {
  todo!()
}

pub async fn get_object_metadata(
  _pool: &PgPool,
  _path: &str,
) -> Result<AFObjectMetadata, sqlx::Error> {
  todo!()
}
