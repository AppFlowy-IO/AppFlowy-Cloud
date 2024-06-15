use crate::file::{BucketClient, BucketStorage, ResponseBlob};
use anyhow::anyhow;
use app_error::AppError;
use async_trait::async_trait;
use aws_sdk_s3::operation::delete_object::DeleteObjectOutput;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::Client;
use database_entity::file_dto::{
  CompleteUploadRequest, CreateUploadRequest, CreateUploadResponse, UploadPartRequest,
  UploadPartResponse,
};
use tokio::sync::Mutex;
use tracing::trace;

pub type S3BucketStorage = BucketStorage<AwsS3BucketClientImpl>;

impl S3BucketStorage {
  pub fn from_bucket_impl(client: AwsS3BucketClientImpl, pg_pool: sqlx::PgPool) -> Self {
    Self::new(client, pg_pool)
  }
}

pub struct AwsS3BucketClientImpl {
  client: Client,
  bucket: String,
  upload_sessions: Arc<Mutex<HashMap<String, Vec<CompletedPart>>>>,
}

impl AwsS3BucketClientImpl {
  pub fn new(client: Client, bucket: String) -> Self {
    debug_assert!(!bucket.is_empty());
    AwsS3BucketClientImpl {
      client,
      bucket,
      upload_sessions: Arc::new(Default::default()),
    }
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
    trace!(
      "Uploading object to S3 bucket:{}, key {}, len: {}",
      self.bucket,
      key,
      content.len()
    );
    let body = ByteStream::from(content.to_vec());
    self
      .client
      .put_object()
      .bucket(&self.bucket)
      .key(key)
      .body(body)
      .send()
      .await
      .map_err(|err| anyhow!("Failed to upload object to S3: {}", err))?;

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
      .map_err(|err| anyhow!("Failed to delete object to S3: {}", err))?;

    Ok(S3ResponseData::new(output))
  }

  async fn get_blob<P>(&self, id: P) -> Result<Self::ResponseData, AppError>
  where
    P: AsRef<str> + Send,
  {
    let key = id.as_ref().to_string();

    match self
      .client
      .get_object()
      .bucket(&self.bucket)
      .key(key)
      .send()
      .await
    {
      Ok(output) => match output.body.collect().await {
        Ok(body) => {
          let data = body.into_bytes().to_vec();
          Ok(S3ResponseData::new_with_data(data))
        },
        Err(err) => Err(AppError::from(anyhow!("Failed to collect body: {}", err))),
      },
      Err(SdkError::ServiceError(service_err)) => match service_err.err() {
        GetObjectError::NoSuchKey(_) => Err(AppError::RecordNotFound("blob not found".to_string())),
        _ => Err(AppError::from(anyhow!(
          "Failed to get object from S3: {:?}",
          service_err
        ))),
      },
      Err(err) => Err(AppError::from(anyhow!(
        "Failed to get object from S3: {}",
        err
      ))),
    }
  }

  async fn create_upload(
    &self,
    req: CreateUploadRequest,
  ) -> Result<CreateUploadResponse, AppError> {
    let multipart_upload_res = self
      .client
      .create_multipart_upload()
      .bucket(&self.bucket)
      .key(&req.key)
      .send()
      .await
      .map_err(|err| anyhow!("Failed to create upload: {}", err))?;

    match multipart_upload_res.upload_id {
      None => Err(anyhow!("Failed to create upload: upload_id is None").into()),
      Some(upload_id) => Ok(CreateUploadResponse {
        key: req.key,
        upload_id,
      }),
    }
  }

  async fn upload_part(&self, req: UploadPartRequest) -> Result<UploadPartResponse, AppError> {
    let body = ByteStream::from(req.body);
    let upload_part_res = self
      .client
      .upload_part()
      .bucket(&self.bucket)
      .key(&req.key)
      .upload_id(&req.upload_id)
      .part_number(req.part_number)
      .body(body)
      .send()
      .await
      .map_err(|err| anyhow!("Failed to upload part: {}", err))?;

    match upload_part_res.e_tag {
      None => Err(anyhow!("Failed to upload part: e_tag is None").into()),
      Some(e_tag) => {
        let mut sessions = self.upload_sessions.lock().await;
        let parts = sessions
          .entry(req.upload_id.clone())
          .or_insert_with(Vec::new);
        parts.push(
          CompletedPart::builder()
            .e_tag(e_tag.clone())
            .part_number(req.part_number)
            .build(),
        );
        Ok(UploadPartResponse {
          part_num: req.part_number,
          e_tag,
        })
      },
    }
  }

  async fn complete_upload(&self, req: CompleteUploadRequest) -> Result<(), AppError> {
    let parts = self
      .upload_sessions
      .lock()
      .await
      .remove(&req.upload_id)
      .unwrap_or_else(Vec::new);
    let completed_multipart_upload = CompletedMultipartUpload::builder()
      .set_parts(Some(parts))
      .build();

    self
      .client
      .complete_multipart_upload()
      .bucket(&self.bucket)
      .key(&req.key)
      .upload_id(&req.upload_id)
      .multipart_upload(completed_multipart_upload)
      .send()
      .await
      .unwrap();

    Ok(())
  }
}

#[derive(Debug)]
pub struct S3ResponseData {
  data: Vec<u8>,
}

impl Deref for S3ResponseData {
  type Target = Vec<u8>;

  fn deref(&self) -> &Self::Target {
    &self.data
  }
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
