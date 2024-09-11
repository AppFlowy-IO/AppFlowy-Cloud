use std::sync::Arc;

use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::{PublishCollabItem, PublishInfo};
use shared_entity::dto::publish_dto::PublishViewMetaData;
use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

use database::{
  file::{s3_client_impl::AwsS3BucketClientImpl, BucketClient, ResponseBlob},
  publish::{
    delete_published_collabs, insert_or_replace_publish_collabs, select_publish_collab_meta,
    select_published_collab_blob, select_published_collab_info,
    select_published_collab_workspace_view_id, select_published_data_for_view_id,
    select_published_metadata_for_view_id, select_user_is_collab_publisher_for_all_views,
    select_workspace_publish_namespace, select_workspace_publish_namespace_exists,
    update_workspace_publish_namespace,
  },
  workspace::select_user_is_workspace_owner,
};

use crate::api::metrics::PublishedCollabMetrics;

use super::ops::check_workspace_owner;

async fn check_workspace_owner_or_publisher(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
  view_id: &[Uuid],
) -> Result<(), AppError> {
  let is_owner = select_user_is_workspace_owner(pg_pool, user_uuid, workspace_id).await?;
  if !is_owner {
    let is_publisher =
      select_user_is_collab_publisher_for_all_views(pg_pool, user_uuid, workspace_id, view_id)
        .await?;
    if !is_publisher {
      return Err(AppError::UserUnAuthorized(
        "User is not the owner of the workspace or the publisher of the document".to_string(),
      ));
    }
  }
  Ok(())
}

fn check_collab_publish_name(publish_name: &str) -> Result<(), AppError> {
  // Check len
  if publish_name.len() > 128 {
    return Err(AppError::InvalidRequest(
      "Publish name must be at most 128 characters long".to_string(),
    ));
  }

  // Only contain alphanumeric characters and hyphens
  for c in publish_name.chars() {
    if !c.is_alphanumeric() && c != '-' {
      return Err(AppError::InvalidRequest(
        "Publish name must only contain alphanumeric characters and hyphens".to_string(),
      ));
    }
  }

  Ok(())
}

fn get_collab_s3_key(workspace_id: &Uuid, view_id: &Uuid) -> String {
  format!("published-collab/{}/{}", workspace_id, view_id)
}

pub async fn set_workspace_namespace(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
  new_namespace: &str,
) -> Result<(), AppError> {
  check_workspace_owner(pg_pool, user_uuid, workspace_id).await?;
  check_workspace_namespace(new_namespace).await?;
  if select_workspace_publish_namespace_exists(pg_pool, workspace_id, new_namespace).await? {
    return Err(AppError::PublishNamespaceAlreadyTaken(
      "publish namespace is already taken".to_string(),
    ));
  };
  update_workspace_publish_namespace(pg_pool, workspace_id, new_namespace).await?;
  Ok(())
}

pub async fn get_workspace_publish_namespace(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<String, AppError> {
  select_workspace_publish_namespace(pg_pool, workspace_id).await
}

async fn check_workspace_namespace(new_namespace: &str) -> Result<(), AppError> {
  // Check len
  if new_namespace.len() < 8 {
    return Err(AppError::InvalidRequest(
      "Namespace must be at least 8 characters long".to_string(),
    ));
  }

  if new_namespace.len() > 64 {
    return Err(AppError::InvalidRequest(
      "Namespace must be at most 32 characters long".to_string(),
    ));
  }

  // Only contain alphanumeric characters and hyphens
  for c in new_namespace.chars() {
    if !c.is_alphanumeric() && c != '-' {
      return Err(AppError::InvalidRequest(
        "Namespace must only contain alphanumeric characters and hyphens".to_string(),
      ));
    }
  }

  // TODO: add more checks for reserved words

  Ok(())
}

#[async_trait]
pub trait PublishedCollabStore: Sync + Send + 'static {
  async fn publish_collabs(
    &self,
    published_items: Vec<PublishCollabItem<serde_json::Value, Vec<u8>>>,
    workspace_id: &Uuid,
    user_uuid: &Uuid,
  ) -> Result<(), AppError>;

  async fn get_collab_with_view_metadata_by_view_id(
    &self,
    view_id: &Uuid,
  ) -> Result<Option<(PublishViewMetaData, Vec<u8>)>, AppError>;

  async fn get_collab_metadata(
    &self,
    publish_namespace: &str,
    publish_name: &str,
  ) -> Result<serde_json::Value, AppError>;

  async fn get_collab_publish_info(&self, view_id: &Uuid) -> Result<PublishInfo, AppError>;

  async fn get_collab_blob_by_publish_namespace(
    &self,
    publish_namespace: &str,
    publish_name: &str,
  ) -> Result<Vec<u8>, AppError>;

  async fn delete_collab(
    &self,
    workspace_id: &Uuid,
    view_ids: &[Uuid],
    user_uuid: &Uuid,
  ) -> Result<(), AppError>;
}

pub struct PublishedCollabPostgresStore {
  metrics: Arc<PublishedCollabMetrics>,
  pg_pool: PgPool,
}

impl PublishedCollabPostgresStore {
  pub fn new(metrics: Arc<PublishedCollabMetrics>, pg_pool: PgPool) -> Self {
    Self { metrics, pg_pool }
  }
}

#[async_trait]
impl PublishedCollabStore for PublishedCollabPostgresStore {
  async fn publish_collabs(
    &self,
    publish_items: Vec<PublishCollabItem<serde_json::Value, Vec<u8>>>,
    workspace_id: &Uuid,
    user_uuid: &Uuid,
  ) -> Result<(), AppError> {
    for publish_item in &publish_items {
      check_collab_publish_name(publish_item.meta.publish_name.as_str())?;
    }
    let publish_items_batch_size = publish_items.len() as i64;
    let result =
      insert_or_replace_publish_collabs(&self.pg_pool, workspace_id, user_uuid, publish_items)
        .await;
    if result.is_err() {
      self
        .metrics
        .incr_failure_write_count(publish_items_batch_size);
    } else {
      self
        .metrics
        .incr_success_write_count(publish_items_batch_size);
    }
    result
  }

  async fn get_collab_metadata(
    &self,
    publish_namespace: &str,
    publish_name: &str,
  ) -> Result<serde_json::Value, AppError> {
    let metadata =
      select_publish_collab_meta(&self.pg_pool, publish_namespace, publish_name).await?;
    Ok(metadata)
  }

  async fn get_collab_with_view_metadata_by_view_id(
    &self,
    view_id: &Uuid,
  ) -> Result<Option<(PublishViewMetaData, Vec<u8>)>, AppError> {
    let result = match select_published_data_for_view_id(&self.pg_pool, view_id).await? {
      Some((js_val, blob)) => {
        let metadata = serde_json::from_value(js_val)?;
        Ok(Some((metadata, blob)))
      },
      None => Ok(None),
    };
    if result.is_err() {
      self.metrics.incr_failure_read_count(1);
    } else {
      self.metrics.incr_success_read_count(1);
    }
    result
  }

  async fn get_collab_publish_info(&self, view_id: &Uuid) -> Result<PublishInfo, AppError> {
    select_published_collab_info(&self.pg_pool, view_id).await
  }

  async fn get_collab_blob_by_publish_namespace(
    &self,
    publish_namespace: &str,
    publish_name: &str,
  ) -> Result<Vec<u8>, AppError> {
    let result = select_published_collab_blob(&self.pg_pool, publish_namespace, publish_name).await;
    if result.is_err() {
      self.metrics.incr_failure_read_count(1);
    } else {
      self.metrics.incr_success_read_count(1);
    }
    result
  }

  async fn delete_collab(
    &self,
    workspace_id: &Uuid,
    view_ids: &[Uuid],
    user_uuid: &Uuid,
  ) -> Result<(), AppError> {
    check_workspace_owner_or_publisher(&self.pg_pool, user_uuid, workspace_id, view_ids).await?;
    delete_published_collabs(&self.pg_pool, workspace_id, view_ids).await?;
    Ok(())
  }
}

pub struct PublishedCollabS3StoreWithPostgresFallback {
  metrics: Arc<PublishedCollabMetrics>,
  pg_pool: PgPool,
  bucket_client: AwsS3BucketClientImpl,
}

impl PublishedCollabS3StoreWithPostgresFallback {
  pub fn new(
    metrics: Arc<PublishedCollabMetrics>,
    pg_pool: PgPool,
    bucket_client: AwsS3BucketClientImpl,
  ) -> Self {
    Self {
      metrics,
      pg_pool,
      bucket_client,
    }
  }
}

#[async_trait]
impl PublishedCollabStore for PublishedCollabS3StoreWithPostgresFallback {
  async fn publish_collabs(
    &self,
    publish_items: Vec<PublishCollabItem<serde_json::Value, Vec<u8>>>,
    workspace_id: &Uuid,
    user_uuid: &Uuid,
  ) -> Result<(), AppError> {
    let publish_items_batch_size = publish_items.len() as i64;
    let mut handles: Vec<tokio::task::JoinHandle<()>> = vec![];
    for publish_item in &publish_items {
      check_collab_publish_name(publish_item.meta.publish_name.as_str())?;
      let object_key = get_collab_s3_key(workspace_id, &publish_item.meta.view_id);
      let data = publish_item.data.clone();
      let bucket_client = self.bucket_client.clone();
      let metrics = self.metrics.clone();
      let handle = tokio::spawn(async move {
        let result = bucket_client.put_blob(&object_key, &data).await;
        if let Err(err) = result {
          debug!("Failed to publish collab to S3: {}", err);
        } else {
          metrics.incr_success_write_count(1);
        }
      });
      handles.push(handle);
    }
    for handle in handles {
      handle.await?;
    }

    let result =
      insert_or_replace_publish_collabs(&self.pg_pool, workspace_id, user_uuid, publish_items)
        .await;
    if result.is_err() {
      self
        .metrics
        .incr_failure_write_count(publish_items_batch_size);
    } else {
      self
        .metrics
        .incr_fallback_write_count(publish_items_batch_size);
    }
    result
  }

  async fn get_collab_metadata(
    &self,
    publish_namespace: &str,
    publish_name: &str,
  ) -> Result<serde_json::Value, AppError> {
    let metadata =
      select_publish_collab_meta(&self.pg_pool, publish_namespace, publish_name).await?;
    Ok(metadata)
  }

  async fn get_collab_with_view_metadata_by_view_id(
    &self,
    view_id: &Uuid,
  ) -> Result<Option<(PublishViewMetaData, Vec<u8>)>, AppError> {
    let result = select_published_metadata_for_view_id(&self.pg_pool, view_id).await?;
    match result {
      Some((workspace_id, js_val)) => {
        let metadata = serde_json::from_value(js_val)?;
        let object_key = get_collab_s3_key(&workspace_id, view_id);
        match self.bucket_client.get_blob(&object_key).await {
          Ok(resp) => {
            self.metrics.incr_success_read_count(1);
            Ok(Some((metadata, resp.to_blob())))
          },
          Err(_) => {
            let result = match select_published_data_for_view_id(&self.pg_pool, view_id).await? {
              Some((js_val, blob)) => {
                let metadata = serde_json::from_value(js_val)?;
                Ok(Some((metadata, blob)))
              },
              None => Ok(None),
            };
            if result.is_err() {
              self.metrics.incr_failure_read_count(1);
            } else {
              self.metrics.incr_fallback_read_count(1);
            }
            result
          },
        }
      },
      None => {
        self.metrics.incr_success_read_count(1);
        Ok(None)
      },
    }
  }

  async fn get_collab_publish_info(&self, view_id: &Uuid) -> Result<PublishInfo, AppError> {
    select_published_collab_info(&self.pg_pool, view_id).await
  }

  async fn get_collab_blob_by_publish_namespace(
    &self,
    publish_namespace: &str,
    publish_name: &str,
  ) -> Result<Vec<u8>, AppError> {
    let collab_key =
      select_published_collab_workspace_view_id(&self.pg_pool, publish_namespace, publish_name)
        .await?;
    let object_key = get_collab_s3_key(&collab_key.workspace_id, &collab_key.view_id);
    let resp = self.bucket_client.get_blob(&object_key).await;
    match resp {
      Ok(resp) => {
        self.metrics.incr_success_read_count(1);
        Ok(resp.to_blob())
      },
      Err(err) => {
        debug!(
          "Failed to get published collab blob {} from S3 due to {}",
          object_key, err
        );
        let result =
          select_published_collab_blob(&self.pg_pool, publish_namespace, publish_name).await;
        if result.is_err() {
          self.metrics.incr_failure_read_count(1);
        } else {
          self.metrics.incr_fallback_read_count(1);
        }
        result
      },
    }
  }

  async fn delete_collab(
    &self,
    workspace_id: &Uuid,
    view_ids: &[Uuid],
    user_uuid: &Uuid,
  ) -> Result<(), AppError> {
    check_workspace_owner_or_publisher(&self.pg_pool, user_uuid, workspace_id, view_ids).await?;
    let object_keys = view_ids
      .iter()
      .map(|view_id| get_collab_s3_key(workspace_id, view_id))
      .collect::<Vec<String>>();
    self.bucket_client.delete_blobs(object_keys).await?;
    delete_published_collabs(&self.pg_pool, workspace_id, view_ids).await?;
    Ok(())
  }
}
