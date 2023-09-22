use s3::request::ResponseData;
use shared_entity::{error::AppError, error_code::ErrorCode};
use sqlx::types::uuid;

pub async fn create_object_for_user(
  s3_bucket: &s3::Bucket,
  _user_uuid: &uuid::Uuid,
  path: &str,
  data: &[u8],
  _mime: mime::Mime,
) -> Result<(), AppError> {
  // TODO:
  // 1. use user_uuid to check if user has permission to create object
  // 2. how to handle mime?

  let resp = s3_bucket.put_object(path, data).await?;
  check_s3_status(resp)?;
  Ok(())
}

fn check_s3_status(resp: ResponseData) -> Result<(), AppError> {
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
