use crate::error::WorkerError;
use anyhow::{anyhow, Context};
use aws_sdk_s3::error::SdkError;
use std::fs::Permissions;

use anyhow::Result;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::primitives::ByteStream;
use axum::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use futures::AsyncReadExt;
use std::ops::Deref;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{error, info, trace};
use uuid::Uuid;

#[async_trait]
pub trait S3Client: Send + Sync {
  async fn get_blob_stream(&self, object_key: &str) -> Result<S3StreamResponse, WorkerError>;
  async fn put_blob(
    &self,
    object_key: &str,
    content: ByteStream,
    content_type: Option<&str>,
  ) -> Result<(), WorkerError>;
  async fn delete_blob(&self, object_key: &str) -> Result<(), WorkerError>;
}

#[derive(Clone, Debug)]
pub struct S3ClientImpl {
  pub inner: aws_sdk_s3::Client,
  pub bucket: String,
}

impl Deref for S3ClientImpl {
  type Target = aws_sdk_s3::Client;

  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}

#[async_trait]
impl S3Client for S3ClientImpl {
  async fn get_blob_stream(&self, object_key: &str) -> Result<S3StreamResponse, WorkerError> {
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
        let content_length = output.content_length;
        Ok(S3StreamResponse {
          stream: Box::new(stream),
          content_type,
          content_length,
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

  async fn put_blob(
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

  async fn delete_blob(&self, object_key: &str) -> Result<(), WorkerError> {
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
  pub content_length: Option<i64>,
}

pub struct AutoRemoveDownloadedFile {
  zip_file_path: PathBuf,
  pub(crate) workspace_id: String,
}

impl AutoRemoveDownloadedFile {
  pub fn path_buf(&self) -> &PathBuf {
    &self.zip_file_path
  }
}

impl AsRef<Path> for AutoRemoveDownloadedFile {
  fn as_ref(&self) -> &Path {
    &self.zip_file_path
  }
}

impl AsRef<PathBuf> for AutoRemoveDownloadedFile {
  fn as_ref(&self) -> &PathBuf {
    &self.zip_file_path
  }
}

impl Deref for AutoRemoveDownloadedFile {
  type Target = PathBuf;

  fn deref(&self) -> &Self::Target {
    &self.zip_file_path
  }
}

impl Drop for AutoRemoveDownloadedFile {
  fn drop(&mut self) {
    let path = self.zip_file_path.clone();
    let _workspace_id = self.workspace_id.clone();
    tokio::spawn(async move {
      if path.exists() {
        if let Err(err) = fs::remove_file(&path).await {
          error!(
            "Failed to delete the auto remove downloaded file: {:?}, error: {}",
            path, err
          )
        }
      }
    });
  }
}

pub async fn download_file(
  workspace_id: &str,
  storage_dir: &Path,
  stream: Box<dyn futures::AsyncBufRead + Unpin + Send>,
  expected_md5_base64: &Option<String>,
) -> Result<AutoRemoveDownloadedFile, anyhow::Error> {
  let zip_file_dir = storage_dir.join(format!("{}", Uuid::new_v4()));
  if !zip_file_dir.exists() {
    fs::create_dir_all(&zip_file_dir).await?;
    let file_permissions = Permissions::from_mode(0o777);
    fs::set_permissions(&zip_file_dir, file_permissions).await?;
  }

  let zip_file_path = zip_file_dir.join("file.zip");
  trace!(
    "[Import] {} start to write stream to file: {:?}",
    workspace_id,
    zip_file_path
  );
  write_stream_to_file(&zip_file_path, expected_md5_base64, stream).await?;
  info!(
    "[Import] {} finish writing stream to file: {:?}",
    workspace_id, zip_file_path
  );
  Ok(AutoRemoveDownloadedFile {
    zip_file_path,
    workspace_id: workspace_id.to_string(),
  })
}

pub async fn write_stream_to_file(
  file_path: &PathBuf,
  expected_md5_base64: &Option<String>,
  mut stream: Box<dyn futures::AsyncBufRead + Unpin + Send>,
) -> Result<(), anyhow::Error> {
  let mut context = md5::Context::new();
  let mut file = OpenOptions::new()
    .write(true)
    .create(true)
    .truncate(true)
    .mode(0o644)
    .open(file_path)
    .await
    .map_err(|err| anyhow!("Failed to create file with permissions: {:?}", err))?;
  let mut buffer = vec![0u8; 1_048_576];
  loop {
    let bytes_read = stream.read(&mut buffer).await?;
    if bytes_read == 0 {
      break;
    }
    context.consume(&buffer[..bytes_read]);
    file
      .write_all(&buffer[..bytes_read])
      .await
      .with_context(|| format!("Failed to write data to file: {:?}", file_path.as_os_str()))?;
  }

  let digest = context.compute();
  let md5_base64 = STANDARD.encode(digest.as_ref());
  if let Some(expected_md5) = expected_md5_base64 {
    if md5_base64 != *expected_md5 {
      error!(
        "[Import]: MD5 mismatch, expected: {}, current: {}",
        expected_md5, md5_base64
      );
      return Err(anyhow!("MD5 mismatch"));
    }
  }

  file
    .flush()
    .await
    .with_context(|| format!("Failed to flush data to file: {:?}", file_path.as_os_str()))?;
  Ok(())
}
