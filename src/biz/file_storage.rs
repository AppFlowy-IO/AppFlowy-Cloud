use bytes::Bytes;
use s3::request::ResponseData;
use shared_entity::{error::AppError, error_code::ErrorCode};
use sqlx::types::uuid;
use storage::file_storage;

// todo: user writer
pub async fn put_object(
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
  let file_type = mime.to_string();
  let metadata =
    file_storage::insert_file_metadata(&mut trans, user_uuid, path, &file_type, size).await?;
  let resp = s3_bucket.put_object(metadata.s3_path(), data).await?;
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
  match file_storage::delete_file_metadata(&mut trans, user_uuid, path).await {
    Ok(metadata) => {
      let resp = s3_bucket.delete_object(metadata.s3_path()).await?;
      check_s3_status(&resp)?;
      trans.commit().await?;
      Ok(())
    },
    Err(e) => match e {
      sqlx::Error::RowNotFound => Err(ErrorCode::FileNotFound.into()),
      e => Err(e.into()),
    },
  }
}

// user reader
pub async fn get_object(
  pg_pool: &sqlx::PgPool,
  s3_bucket: &s3::Bucket,
  user_uuid: &uuid::Uuid,
  path: &str,
) -> Result<Bytes, AppError> {
  // TODO: access control

  match file_storage::get_file_metadata(pg_pool, user_uuid, path).await {
    Ok(metadata) => {
      let resp = s3_bucket.get_object(metadata.s3_path()).await?;
      check_s3_status(&resp)?;
      Ok(resp.bytes().to_owned())
    },
    Err(e) => match e {
      sqlx::Error::RowNotFound => Err(ErrorCode::FileNotFound.into()),
      e => Err(e.into()),
    },
  }
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
