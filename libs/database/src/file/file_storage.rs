use crate::pg_row::AFBlobMetadataRow;
use crate::resource_usage::{
  delete_blob_metadata, get_blob_metadata, insert_blob_metadata, is_blob_metadata_exists,
};
use app_error::AppError;
use async_trait::async_trait;
use database_entity::file_dto::{
  CompleteUploadRequest, CreateUploadRequest, CreateUploadResponse, UploadPartData,
  UploadPartResponse,
};
use sqlx::PgPool;
use tracing::{info, instrument, warn};
use uuid::Uuid;

pub trait ResponseBlob {
  fn to_blob(self) -> Vec<u8>;
}

#[async_trait]
pub trait BucketClient {
  type ResponseData: ResponseBlob;

  async fn pub_blob<P>(&self, id: &P, content: &[u8]) -> Result<(), AppError>
  where
    P: BlobKey;

  async fn delete_blob(&self, object_key: &str) -> Result<Self::ResponseData, AppError>;

  async fn get_blob(&self, object_key: &str) -> Result<Self::ResponseData, AppError>;

  async fn create_upload(
    &self,
    key: impl BlobKey,
    req: CreateUploadRequest,
  ) -> Result<CreateUploadResponse, AppError>;
  async fn upload_part(
    &self,
    key: &impl BlobKey,
    req: UploadPartData,
  ) -> Result<UploadPartResponse, AppError>;
  async fn complete_upload(
    &self,
    key: &impl BlobKey,
    req: CompleteUploadRequest,
  ) -> Result<(usize, String), AppError>;

  async fn remove_dir(&self, dir: &str) -> Result<(), AppError>;
}

pub trait BlobKey: Send + Sync {
  fn workspace_id(&self) -> &Uuid;
  fn object_key(&self) -> String;
  fn meta_key(&self) -> String;
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
  pub async fn put_blob<K: BlobKey>(
    &self,
    key: K,
    file_data: Vec<u8>,
    file_type: String,
  ) -> Result<(), AppError> {
    if is_blob_metadata_exists(&self.pg_pool, key.workspace_id(), &key.meta_key()).await? {
      warn!(
        "file already exists, workspace_id: {}, meta_key: {}",
        key.workspace_id(),
        key.meta_key()
      );
      return Ok(());
    }

    self.client.pub_blob(&key, &file_data).await?;

    insert_blob_metadata(
      &self.pg_pool,
      &key.meta_key(),
      key.workspace_id(),
      &file_type,
      file_data.len(),
    )
    .await?;
    Ok(())
  }

  pub async fn delete_blob(&self, key: impl BlobKey) -> Result<(), AppError> {
    self.client.delete_blob(&key.object_key()).await?;

    let mut tx = self.pg_pool.begin().await?;
    delete_blob_metadata(&mut tx, key.workspace_id(), &key.meta_key()).await?;
    tx.commit().await?;
    Ok(())
  }

  pub async fn get_blob_metadata(
    &self,
    workspace_id: &Uuid,
    meta_key: &str,
  ) -> Result<AFBlobMetadataRow, AppError> {
    let metadata = get_blob_metadata(&self.pg_pool, workspace_id, meta_key).await?;
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
    self.client.create_upload(key, req).await
  }

  pub async fn upload_part(
    &self,
    key: impl BlobKey,
    req: UploadPartData,
  ) -> Result<UploadPartResponse, AppError> {
    self.client.upload_part(&key, req).await
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

    let (content_length, content_type) = self.client.complete_upload(&key, req).await?;
    insert_blob_metadata(
      &self.pg_pool,
      &key.meta_key(),
      key.workspace_id(),
      &content_type,
      content_length,
    )
    .await?;
    Ok(())
  }
}
