use crate::file::utils::BlobStreamReader;
use crate::resource_usage::{
  delete_blob_metadata, get_blob_metadata, get_workspace_usage_size, insert_blob_metadata,
  is_blob_metadata_exists,
};
use async_trait::async_trait;
use database_entity::error::DatabaseError;
use database_entity::pg_row::AFBlobMetadataRow;
use sqlx::PgPool;
use tokio::io::AsyncRead;
use tracing::{event, instrument};
use uuid::Uuid;

/// Maximum size of a blob in bytes.
pub const MAX_BLOB_SIZE: usize = 6 * 1024 * 1024;
pub const MAX_USAGE: u64 = 10 * 1024 * 1024 * 1024;

pub trait ResponseBlob {
  fn to_blob(self) -> Vec<u8>;
}

#[async_trait]
pub trait BucketClient {
  type ResponseData: ResponseBlob;
  type Error: Into<DatabaseError>;

  async fn put_blob<P>(&self, id: P, blob: Vec<u8>) -> Result<(), Self::Error>
  where
    P: AsRef<str> + Send;

  async fn delete_blob<P>(&self, id: P) -> Result<Self::ResponseData, Self::Error>
  where
    P: AsRef<str> + Send;

  async fn get_blob<P>(&self, id: P) -> Result<Self::ResponseData, Self::Error>
  where
    P: AsRef<str> + Send;
}

pub struct BucketStorage<C> {
  client: C,
  pg_pool: PgPool,
}

impl<C> BucketStorage<C>
where
  C: BucketClient,
  DatabaseError: From<<C as BucketClient>::Error>,
{
  pub fn new(client: C, pg_pool: PgPool) -> Self {
    Self { client, pg_pool }
  }

  #[instrument(skip_all, err)]
  pub async fn put_blob<R>(
    &self,
    blob_stream: R,
    workspace_id: Uuid,
    file_type: String,
    file_size: i64,
  ) -> Result<String, DatabaseError>
  where
    R: AsyncRead + Unpin,
  {
    let (blob, file_id) = BlobStreamReader::new(blob_stream).finish().await?;

    // check file is exist or not
    if is_blob_metadata_exists(&self.pg_pool, &workspace_id, &file_id).await? {
      event!(tracing::Level::TRACE, "file:{} is already exist", file_id);
      return Ok(file_id);
    }

    // query the storage space of the workspace
    let usage = get_workspace_usage_size(&self.pg_pool, &workspace_id).await?;
    event!(
      tracing::Level::TRACE,
      "workspace consumed space: {}, new file:{} with size: {}",
      usage,
      file_id,
      file_size
    );
    if usage > MAX_USAGE {
      return Err(DatabaseError::StorageSpaceNotEnough);
    }

    self.client.put_blob(&file_id, blob).await?;

    // save the metadata
    if let Err(err) = insert_blob_metadata(
      &self.pg_pool,
      &file_id,
      &workspace_id,
      &file_type,
      file_size,
    )
    .await
    {
      event!(
        tracing::Level::ERROR,
        "failed to save metadata, file_id: {}, err: {}",
        file_id,
        err
      );
      // if the metadata is not saved, delete the blob.
      self.client.delete_blob(&file_id).await?;
      return Err(err);
    }

    Ok(file_id)
  }

  pub async fn delete_blob(
    &self,
    workspace_id: &Uuid,
    file_id: &str,
  ) -> Result<AFBlobMetadataRow, DatabaseError> {
    self.client.delete_blob(file_id).await?;
    let resp = delete_blob_metadata(&self.pg_pool, workspace_id, file_id).await?;
    Ok(resp)
  }

  pub async fn get_blob_metadata(
    &self,
    workspace_id: &Uuid,
    file_id: &str,
  ) -> Result<AFBlobMetadataRow, DatabaseError> {
    let metadata = get_blob_metadata(&self.pg_pool, workspace_id, file_id).await?;
    Ok(metadata)
  }

  pub async fn get_blob(&self, file_id: &str) -> Result<Vec<u8>, DatabaseError> {
    let blob = self.client.get_blob(file_id).await?.to_blob();
    Ok(blob)
  }
}
