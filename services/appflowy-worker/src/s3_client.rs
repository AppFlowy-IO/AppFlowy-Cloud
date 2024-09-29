use crate::error::WorkerError;
use anyhow::anyhow;
use aws_sdk_s3::error::SdkError;

use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::primitives::ByteStream;
use std::ops::Deref;
use tokio_util::compat::TokioAsyncReadCompatExt;

#[derive(Clone, Debug)]
pub struct S3Client {
  pub inner: aws_sdk_s3::Client,
  pub bucket: String,
}

impl Deref for S3Client {
  type Target = aws_sdk_s3::Client;

  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}

impl S3Client {
  pub(crate) async fn get_blob(&self, object_key: &str) -> Result<S3StreamResponse, WorkerError> {
    match self
      .inner
      .get_object()
      .bucket(&self.bucket)
      .key(object_key)
      .send()
      .await
    {
      Ok(output) => {
        let stream = output.body.into_async_read().compat();
        let content_type = output.content_type;
        Ok(S3StreamResponse {
          stream: Box::new(stream),
          content_type,
        })
      },
      Err(SdkError::ServiceError(service_err)) => match service_err.err() {
        GetObjectError::NoSuchKey(_) => Err(WorkerError::RecordNotFound(format!(
          "blob not found for key:{object_key}"
        ))),
        _ => Err(WorkerError::from(anyhow!(
          "Failed to get object from S3: {:?}",
          service_err
        ))),
      },
      Err(err) => Err(WorkerError::from(anyhow!(
        "Failed to get object from S3: {}",
        err
      ))),
    }
  }

  pub(crate) async fn put_blob(
    &self,
    object_key: &str,
    content: ByteStream,
    content_type: Option<&str>,
  ) -> Result<(), WorkerError> {
    match self
      .inner
      .put_object()
      .bucket(&self.bucket)
      .key(object_key)
      .body(content)
      .content_type(content_type.unwrap_or("application/octet-stream"))
      .send()
      .await
    {
      Ok(_) => Ok(()),
      Err(err) => Err(WorkerError::from(anyhow!(
        "Failed to put object to S3: {}",
        err
      ))),
    }
  }

  pub(crate) async fn delete_blob(&self, object_key: &str) -> Result<(), WorkerError> {
    match self
      .inner
      .delete_object()
      .bucket(&self.bucket)
      .key(object_key)
      .send()
      .await
    {
      Ok(_) => Ok(()),
      Err(SdkError::ServiceError(service_err)) => Err(WorkerError::from(anyhow!(
        "Failed to delete object from S3: {:?}",
        service_err
      ))),
      Err(err) => Err(WorkerError::from(anyhow!(
        "Failed to delete object from S3: {}",
        err
      ))),
    }
  }
}

pub struct S3StreamResponse {
  pub stream: Box<dyn futures::AsyncBufRead + Unpin + Send>,
  pub content_type: Option<String>,
}
