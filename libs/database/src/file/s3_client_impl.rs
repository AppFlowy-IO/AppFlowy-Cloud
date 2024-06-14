use crate::file::{BucketClient, ResponseBlob};
use app_error::AppError;
use async_trait::async_trait;
use aws_sdk_s3::operation::delete_object::DeleteObjectOutput;

use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;

pub struct AwsS3BucketClientImpl {
  client: Client,
  bucket: String,
}

impl AwsS3BucketClientImpl {
  pub fn new(client: Client, bucket: String) -> Self {
    AwsS3BucketClientImpl { client, bucket }
  }
}

#[async_trait]
impl BucketClient for AwsS3BucketClientImpl {
  type ResponseData = S3ResponseData;

  async fn pub_blob<P>(&self, id: P, content: &[u8]) -> Result<(), AppError>
  where
    P: AsRef<str> + Send,
  {
    let key = id.as_ref().to_string();
    let body = ByteStream::from(content.to_vec());

    self
      .client
      .put_object()
      .bucket(&self.bucket)
      .key(key)
      .body(body)
      .send()
      .await
      .map_err(anyhow::Error::from)?;
    Ok(())
  }

  async fn delete_blob<P>(&self, id: P) -> Result<Self::ResponseData, AppError>
  where
    P: AsRef<str> + Send,
  {
    let key = id.as_ref().to_string();
    let output = self
      .client
      .delete_object()
      .bucket(&self.bucket)
      .key(key)
      .send()
      .await
      .map_err(anyhow::Error::from)?;

    Ok(S3ResponseData::new(output))
  }

  async fn get_blob<P>(&self, id: P) -> Result<Self::ResponseData, AppError>
  where
    P: AsRef<str> + Send,
  {
    let key = id.as_ref().to_string();

    let output = self
      .client
      .get_object()
      .bucket(&self.bucket)
      .key(key)
      .send()
      .await
      .map_err(anyhow::Error::from)?;

    let body = output.body.collect().await.map_err(anyhow::Error::from)?;
    let data = body.into_bytes().to_vec();

    Ok(S3ResponseData::new_with_data(data))
  }
}

pub struct S3ResponseData {
  data: Vec<u8>,
}
impl ResponseBlob for S3ResponseData {
  fn to_blob(self) -> Vec<u8> {
    self.data
  }
}

impl S3ResponseData {
  pub fn new(_output: DeleteObjectOutput) -> Self {
    S3ResponseData { data: Vec::new() }
  }

  pub fn new_with_data(data: Vec<u8>) -> Self {
    S3ResponseData { data }
  }
}
