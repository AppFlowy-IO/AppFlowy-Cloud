use bytes::Bytes;
use s3::request::ResponseData;
use shared_entity::{error::AppError, error_code::ErrorCode};
use sqlx::types::uuid;
use storage::{file_storage, user::get_user_id};

use super::utils::CountingReader;

pub async fn put_object(
  pg_pool: &sqlx::PgPool,
  s3_bucket: &s3::Bucket,
  user_uuid: &uuid::Uuid,
  file_path: &str,
  mime: mime::Mime,
  async_read: &mut (impl tokio::io::AsyncRead + std::marker::Unpin),
) -> Result<(), AppError> {
  // TODO: access control

  let file_type = mime.to_string();
  let owner_uid = get_user_id(pg_pool, user_uuid).await?;
  let full_path = format!("{}/{}", owner_uid, file_path);
  let mut counting_reader = CountingReader::new(async_read);
  let status_code = s3_bucket
    .put_object_stream(&mut counting_reader, full_path)
    .await?;
  check_s3_status_code(status_code)?;

  let size = counting_reader.count();
  let _metadata =
    file_storage::insert_file_metadata(pg_pool, owner_uid, file_path, &file_type, size as i64)
      .await?;
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
      check_s3_response_data(&resp)?;
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
      check_s3_response_data(&resp)?;
      Ok(resp.bytes().to_owned())
    },
    Err(e) => match e {
      sqlx::Error::RowNotFound => Err(ErrorCode::FileNotFound.into()),
      e => Err(e.into()),
    },
  }
}

fn check_s3_response_data(resp: &ResponseData) -> Result<(), AppError> {
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

fn check_s3_status_code(status_code: u16) -> Result<(), AppError> {
  match status_code {
    200..=299 => Ok(()),
    error_code => {
      tracing::error!("S3 error: {}", error_code);
      Err(ErrorCode::S3Error.into())
    },
  }
}
