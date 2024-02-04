use crate::file::{BucketClient, BucketStorage, ResponseBlob};
use app_error::AppError;
use async_trait::async_trait;

pub type S3BucketStorage = BucketStorage<BucketClientS3Impl>;

impl S3BucketStorage {
  pub fn from_s3_bucket(bucket: s3::Bucket, pg_pool: sqlx::PgPool) -> Self {
    Self::new(BucketClientS3Impl(bucket), pg_pool)
  }
}

pub struct BucketClientS3Impl(s3::Bucket);

#[async_trait]
impl BucketClient for BucketClientS3Impl {
  type ResponseData = S3ResponseData;

  async fn pub_blob<P>(&self, id: P, content: &[u8]) -> Result<(), AppError>
  where
    P: AsRef<str> + Send,
  {
    let code = self.0.put_object(id, content).await?.status_code();
    check_s3_status_code(code)?;
    Ok(())
  }

  async fn delete_blob<P>(&self, id: P) -> Result<Self::ResponseData, AppError>
  where
    P: AsRef<str> + Send,
  {
    let response = self.0.delete_object(id).await?;
    check_s3_response_data(&response)?;
    Ok(S3ResponseData(response))
  }

  async fn get_blob<P>(&self, id: P) -> Result<Self::ResponseData, AppError>
  where
    P: AsRef<str> + Send,
  {
    let response = self.0.get_object(id).await?;
    Ok(S3ResponseData(response))
  }
}

pub struct S3ResponseData(s3::request::ResponseData);
impl ResponseBlob for S3ResponseData {
  fn to_blob(self) -> Vec<u8> {
    self.0.to_vec()
  }
}

#[inline]
fn check_s3_response_data(resp: &s3::request::ResponseData) -> Result<(), AppError> {
  let status_code = resp.status_code();
  match status_code {
    200..=299 => Ok(()),
    error_code => {
      let text = resp.bytes();
      let s = String::from_utf8_lossy(text);
      let msg = format!("S3 error: {}, code: {}", s, error_code);
      Err(AppError::S3ResponseError(msg))
    },
  }
}

#[inline]
fn check_s3_status_code(status_code: u16) -> Result<(), AppError> {
  match status_code {
    200..=299 => Ok(()),
    error_code => Err(AppError::S3ResponseError(format!("{}", error_code))),
  }
}
