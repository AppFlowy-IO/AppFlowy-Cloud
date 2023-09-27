use std::pin::Pin;

use futures_util::Stream;

use bytes::Bytes;
use s3::request::{ResponseData, ResponseDataStream};
use shared_entity::{error::AppError, error_code::ErrorCode};
use sqlx::types::uuid;
use storage::{file_storage, user::get_user_id};
use tokio_stream::StreamExt;

use super::utils::CountingReader;

pub async fn put_object<R>(
  pg_pool: &sqlx::PgPool,
  s3_bucket: &s3::Bucket,
  user_uuid: &uuid::Uuid,
  file_path: &str,
  mime: mime::Mime,
  async_read: &mut R,
) -> Result<(), AppError>
where
  R: tokio::io::AsyncRead + std::marker::Unpin,
{
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

pub async fn get_object(
  pg_pool: &sqlx::PgPool,
  s3_bucket: &s3::Bucket,
  user_uuid: &uuid::Uuid,
  path: &str,
) -> Result<Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>>>>, AppError> {
  // TODO: access control

  match file_storage::get_file_metadata(pg_pool, user_uuid, path).await {
    Ok(metadata) => {
      let resp = s3_bucket.get_object_stream(metadata.s3_path()).await?;
      Ok(s3_response_stream_to_tokio_stream(resp))
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

fn s3_response_stream_to_tokio_stream(
  resp: ResponseDataStream,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>>>> {
  let mapped = resp.bytes.map(Ok::<bytes::Bytes, std::io::Error>);
  Box::pin(mapped)
}
