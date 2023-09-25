use bytes::Bytes;
use s3::request::ResponseData;
use shared_entity::{error::AppError, error_code::ErrorCode};
use sqlx::types::uuid;
use storage::file_storage;

pub async fn create_object(
  pg_pool: &sqlx::PgPool,
  s3_bucket: &s3::Bucket,
  user_uuid: &uuid::Uuid,
  path: &str,
  data: &[u8],
  mime: mime::Mime,
) -> Result<(), AppError> {
  // TODO: access control

  let size = data.len() as i64;
  let mut trans = pg_pool.begin().await?;
  let s3_key = uuid::Uuid::new_v4();
  let file_type = mime.to_string();
  file_storage::insert_object_metadata(&mut trans, user_uuid, path, &file_type, size, &s3_key)
    .await?;

  let resp = s3_bucket.put_object(s3_key.to_string(), data).await?;

  check_s3_status(&resp)?;
  trans.commit().await?;
  Ok(())
}

pub async fn delete_object(
  pg_pool: &sqlx::PgPool,
  s3_bucket: &s3::Bucket,
  user_uuid: &uuid::Uuid,
  path: &str,
) -> Result<(), AppError> {
  // TODO: access control

  let mut trans = pg_pool.begin().await?;
  let s3_key = file_storage::set_delete_object_metadata(&mut trans, user_uuid, path).await?;
  let resp = s3_bucket.delete_object(s3_key).await?;

  check_s3_status(&resp)?;
  trans.commit().await?;
  Ok(())
}

pub async fn get_object(
  pg_pool: &sqlx::PgPool,
  s3_bucket: &s3::Bucket,
  _user_uuid: &uuid::Uuid,
  path: &str,
) -> Result<Bytes, AppError> {
  // TODO: access control

  let object_metadata = file_storage::get_object_metadata(pg_pool, path).await?;
  let resp = s3_bucket
    .get_object(&object_metadata.s3_key.to_string())
    .await?;
  check_s3_status(&resp)?;
  Ok(resp.bytes().to_owned())
}

fn check_s3_status(resp: &ResponseData) -> Result<(), AppError> {
  let status_code = resp.status_code();
  match status_code {
    200..=299 => Ok(()),
    error_code => {
      let text = resp.bytes();
      let s = String::from_utf8_lossy(text);
      Err(AppError::new(
        ErrorCode::S3Error,
        format!("{}: {}", error_code, s),
      ))
    },
  }
}
