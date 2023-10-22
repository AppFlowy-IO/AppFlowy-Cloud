use crate::file::{BucketClient, BucketStorage, ResponseBlob};
use async_trait::async_trait;
use database_entity::error::DatabaseError;
use s3::error::S3Error;

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
  type Error = S3BucketError;

  async fn put_blob<P>(&self, id: P, blob: Vec<u8>) -> Result<(), Self::Error>
  where
    P: AsRef<str> + Send,
  {
    let code = self.0.put_object(id, &blob).await?.status_code();
    check_s3_status_code(code)?;
    Ok(())
  }

  async fn delete_blob<P>(&self, id: P) -> Result<Self::ResponseData, Self::Error>
  where
    P: AsRef<str> + Send,
  {
    let response = self.0.delete_object(id).await?;
    check_s3_response_data(&response)?;
    Ok(S3ResponseData(response))
  }

  async fn get_blob<P>(&self, id: P) -> Result<Self::ResponseData, Self::Error>
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

pub struct S3BucketError(String);
impl From<S3Error> for S3BucketError {
  fn from(value: S3Error) -> Self {
    Self(value.to_string())
  }
}

impl From<S3BucketError> for DatabaseError {
  fn from(value: S3BucketError) -> Self {
    DatabaseError::BucketError(format!("{:?}", value.0))
  }
}

#[inline]
fn check_s3_response_data(resp: &s3::request::ResponseData) -> Result<(), S3BucketError> {
  let status_code = resp.status_code();
  match status_code {
    200..=299 => Ok(()),
    error_code => {
      let text = resp.bytes();
      let s = String::from_utf8_lossy(text);
      Err(S3BucketError(format!(
        "S3 error: {}, code: {}",
        s, error_code
      )))
    },
  }
}

#[inline]
fn check_s3_status_code(status_code: u16) -> Result<(), S3BucketError> {
  match status_code {
    200..=299 => Ok(()),
    error_code => Err(S3BucketError(format!("S3 error: {}", error_code))),
  }
}
