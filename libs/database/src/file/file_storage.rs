use crate::pg_row::AFBlobMetadataRow;
use crate::resource_usage::{
  delete_blob_metadata, get_blob_metadata, insert_blob_metadata, is_blob_metadata_exists,
};
use app_error::AppError;
use async_trait::async_trait;
use sqlx::PgPool;

use tracing::{instrument, warn};
use uuid::Uuid;

pub trait ResponseBlob {
  fn to_blob(self) -> Vec<u8>;
}

#[async_trait]
pub trait BucketClient {
  type ResponseData: ResponseBlob;

  async fn pub_blob<P>(&self, id: P, content: &[u8]) -> Result<(), AppError>
  where
    P: AsRef<str> + Send;

  async fn delete_blob<P>(&self, id: P) -> Result<Self::ResponseData, AppError>
  where
    P: AsRef<str> + Send;

  async fn get_blob<P>(&self, id: P) -> Result<Self::ResponseData, AppError>
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
{
  pub fn new(client: C, pg_pool: PgPool) -> Self {
    Self { client, pg_pool }
  }

  #[instrument(skip_all, err)]
  #[inline]
  pub async fn put_blob(
    &self,
    workspace_id: Uuid,
    file_id: String,
    file_data: Vec<u8>,
    file_type: String,
  ) -> Result<(), AppError> {
    if is_blob_metadata_exists(&self.pg_pool, &workspace_id, &file_id).await? {
      warn!(
        "file already exists, workspace_id: {}, file_id: {}",
        workspace_id, file_id
      );
      return Ok(());
    }

    let obj_key = format!("{}/{}", workspace_id, file_id);
    self.client.pub_blob(obj_key, &file_data).await?;

    insert_blob_metadata(
      &self.pg_pool,
      &file_id,
      &workspace_id,
      &file_type,
      file_data.len(),
    )
    .await?;
    Ok(())
  }

  pub async fn delete_blob(&self, workspace_id: &Uuid, file_id: &str) -> Result<(), AppError> {
    let obj_key = format!("{}/{}", workspace_id, file_id);
    let mut tx = self.pg_pool.begin().await?;
    delete_blob_metadata(&mut tx, workspace_id, file_id).await?;
    self.client.delete_blob(obj_key).await?;
    tx.commit().await?;
    Ok(())
  }

  pub async fn get_blob_metadata(
    &self,
    workspace_id: &Uuid,
    file_id: &str,
  ) -> Result<AFBlobMetadataRow, AppError> {
    let metadata = get_blob_metadata(&self.pg_pool, workspace_id, file_id).await?;
    Ok(metadata)
  }

  pub async fn get_blob(&self, workspace_id: &Uuid, file_id: &str) -> Result<Vec<u8>, AppError> {
    let obj_key = format!("{}/{}", workspace_id, file_id);
    let blob = self.client.get_blob(obj_key).await?.to_blob();
    Ok(blob)
  }
}
