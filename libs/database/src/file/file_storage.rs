use crate::pg_row::AFBlobMetadataRow;
use crate::resource_usage::{
  delete_blob_metadata, get_blob_metadata, insert_blob_metadata, is_blob_metadata_exists,
};
use app_error::AppError;
use async_trait::async_trait;
use aws_sdk_s3::primitives::ByteStream;
use database_entity::file_dto::{
  CompleteUploadRequest, CreateUploadRequest, CreateUploadResponse, UploadPartData,
  UploadPartResponse,
};
use sqlx::PgPool;

use tracing::{info, instrument, warn};
use uuid::Uuid;

pub trait ResponseBlob {
  fn to_blob(self) -> Vec<u8>;
  fn content_type(&self) -> Option<String>;
}

#[async_trait]
pub trait BucketClient {
  type ResponseData: ResponseBlob;

  async fn put_blob(
    &self,
    object_key: &str,
    content: ByteStream,
    content_type: Option<&str>,
  ) -> Result<(), AppError>;

  async fn put_blob_with_content_type(
    &self,
    object_key: &str,
    stream: ByteStream,
    content_type: &str,
  ) -> Result<(), AppError>;

  async fn delete_blob(&self, object_key: &str) -> Result<Self::ResponseData, AppError>;

  async fn delete_blobs(&self, object_key: Vec<String>) -> Result<(), AppError>;

  async fn get_blob(&self, object_key: &str) -> Result<Self::ResponseData, AppError>;

  async fn create_upload(
    &self,
    object_key: &str,
    req: CreateUploadRequest,
  ) -> Result<CreateUploadResponse, AppError>;
  async fn upload_part(
    &self,
    object_key: &str,
    req: UploadPartData,
  ) -> Result<UploadPartResponse, AppError>;
  async fn complete_upload(
    &self,
    object_key: &str,
    req: CompleteUploadRequest,
  ) -> Result<(usize, String), AppError>;

  async fn remove_dir(&self, dir: &str) -> Result<(), AppError>;

  async fn list_dir(&self, dir: &str, limit: usize) -> Result<Vec<String>, AppError>;
}

pub trait BlobKey: Send + Sync {
  fn workspace_id(&self) -> &Uuid;
  fn object_key(&self) -> String;
  fn blob_metadata_key(&self) -> String;
  fn e_tag(&self) -> &str;
}

pub struct BucketStorage<C> {
  client: C,
  pg_pool: PgPool,
}

impl<C> BucketStorage<C>
where
  C: BucketClient,
{
  pub fn new(client: C, pg_pool: PgPool) -> Self {
    Self { client, pg_pool }
  }

  pub async fn remove_dir(&self, dir: &str) -> Result<(), AppError> {
    info!("removing dir: {}", dir);
    self.client.remove_dir(dir).await?;
    Ok(())
  }

  #[instrument(skip_all, err)]
  #[inline]
  pub async fn put_blob_with_content_type<K: BlobKey>(
    &self,
    key: K,
    file_stream: ByteStream,
    file_type: String,
    file_size: usize,
  ) -> Result<(), AppError> {
    if is_blob_metadata_exists(&self.pg_pool, key.workspace_id(), &key.blob_metadata_key()).await? {
      warn!(
        "file already exists, workspace_id: {}, blob_metadata_key: {}",
        key.workspace_id(),
        key.blob_metadata_key()
      );
      return Ok(());
    }

    self
      .client
      .put_blob(&key.object_key(), file_stream, Some(&file_type))
      .await?;
    insert_blob_metadata(
      &self.pg_pool,
      &key.blob_metadata_key(),
      key.workspace_id(),
      &file_type,
      file_size,
    )
    .await?;
    Ok(())
  }

  pub async fn delete_blob(&self, key: impl BlobKey) -> Result<(), AppError> {
    self.client.delete_blob(&key.object_key()).await?;

    let mut tx = self.pg_pool.begin().await?;
    delete_blob_metadata(&mut tx, key.workspace_id(), &key.blob_metadata_key()).await?;
    tx.commit().await?;
    Ok(())
  }

  pub async fn get_blob_metadata(
    &self,
    workspace_id: &Uuid,
    metadata_key: &str,
  ) -> Result<AFBlobMetadataRow, AppError> {
    let metadata = get_blob_metadata(&self.pg_pool, workspace_id, metadata_key).await?;
    Ok(metadata)
  }

  pub async fn get_blob(&self, key: &impl BlobKey) -> Result<Vec<u8>, AppError> {
    let blob = self.client.get_blob(&key.object_key()).await?.to_blob();
    Ok(blob)
  }

  pub async fn create_upload(
    &self,
    key: impl BlobKey,
    req: CreateUploadRequest,
  ) -> Result<CreateUploadResponse, AppError> {
    self.client.create_upload(&key.object_key(), req).await
  }

  pub async fn upload_part(
    &self,
    key: impl BlobKey,
    req: UploadPartData,
  ) -> Result<UploadPartResponse, AppError> {
    self.client.upload_part(&key.object_key(), req).await
  }

  pub async fn complete_upload(
    &self,
    key: impl BlobKey,
    req: CompleteUploadRequest,
  ) -> Result<(), AppError> {
    if is_blob_metadata_exists(&self.pg_pool, key.workspace_id(), &key.object_key()).await? {
      warn!(
        "file already exists, workspace_id: {}, request: {}",
        key.workspace_id(),
        req
      );
      return Ok(());
    }

    let (content_length, content_type) =
      self.client.complete_upload(&key.object_key(), req).await?;
    insert_blob_metadata(
      &self.pg_pool,
      &key.blob_metadata_key(),
      key.workspace_id(),
      &content_type,
      content_length,
    )
    .await?;
    Ok(())
  }
}
