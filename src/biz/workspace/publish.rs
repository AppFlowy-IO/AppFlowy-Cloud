use database::{
  publish::{
    insert_non_orginal_workspace_publish_namespace, select_all_published_collab_info,
    select_default_published_view_id, select_default_published_view_id_for_namespace,
    select_workspace_publish_namespace, select_workspace_publish_namespaces,
    update_published_collabs, update_workspace_default_publish_view,
    update_workspace_default_publish_view_set_null,
  },
  workspace::{select_publish_name_exists, select_view_id_from_publish_name},
};
use database_entity::dto::PatchPublishedCollab;
use std::sync::Arc;

use app_error::AppError;
use async_trait::async_trait;
use aws_sdk_s3::primitives::ByteStream;
use database_entity::dto::{PublishCollabItem, PublishInfo};
use shared_entity::dto::{
  publish_dto::PublishViewMetaData,
  workspace_dto::{FolderViewMinimal, PublishInfoView},
};
use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

use database::{
  file::{s3_client_impl::AwsS3BucketClientImpl, BucketClient, ResponseBlob},
  publish::{
    insert_or_replace_publish_collabs, select_publish_collab_meta, select_published_collab_blob,
    select_published_collab_info, select_published_collab_workspace_view_id,
    select_published_data_for_view_id, select_published_metadata_for_view_id,
    select_user_is_collab_publisher_for_all_views, select_workspace_publish_namespace_exists,
    set_published_collabs_as_unpublished, update_non_orginal_workspace_publish_namespace,
  },
  workspace::select_user_is_workspace_owner,
};

use crate::{
  api::metrics::PublishedCollabMetrics, biz::collab::folder_view::to_dto_folder_view_miminal,
};

use appflowy_collaborate::ws2::WorkspaceCollabInstanceCache;

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
  const MAX_PUBLISH_NAME_LENGTH: usize = 128;

  // Check len
  if publish_name.len() > MAX_PUBLISH_NAME_LENGTH {
    return Err(AppError::PublishNameTooLong {
      given_length: publish_name.len(),
      max_length: MAX_PUBLISH_NAME_LENGTH,
    });
  }

  // Only contain alphanumeric characters and hyphens
  for c in publish_name.chars() {
    if !c.is_alphanumeric() && c != '-' && c != '_' {
      return Err(AppError::PublishNameInvalidCharacter { character: c });
    }
  }

  Ok(())
}

fn get_collab_s3_key(workspace_id: &Uuid, view_id: &Uuid) -> String {
  format!("published-collab/{}/{}", workspace_id, view_id)
}

pub async fn set_workspace_namespace(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  old_namespace: &str,
  new_namespace: &str,
) -> Result<(), AppError> {
  check_workspace_namespace(new_namespace).await?;
  if select_workspace_publish_namespace_exists(pg_pool, new_namespace).await? {
    return Err(AppError::PublishNamespaceAlreadyTaken(
      "publish namespace is already taken".to_string(),
    ));
  };
  let ws_namespace =
    select_workspace_publish_namespace(pg_pool, workspace_id, old_namespace).await?;
  if ws_namespace.is_original {
    insert_non_orginal_workspace_publish_namespace(pg_pool, workspace_id, new_namespace).await?;
  } else {
    update_non_orginal_workspace_publish_namespace(
      pg_pool,
      workspace_id,
      old_namespace,
      new_namespace,
    )
    .await?;
  }
  Ok(())
}

pub async fn set_workspace_default_publish_view(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  new_view_id: &Uuid,
) -> Result<(), AppError> {
  update_workspace_default_publish_view(pg_pool, workspace_id, new_view_id).await?;
  Ok(())
}

pub async fn unset_workspace_default_publish_view(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<(), AppError> {
  update_workspace_default_publish_view_set_null(pg_pool, workspace_id).await?;
  Ok(())
}

pub async fn get_workspace_default_publish_view_info(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<PublishInfo, AppError> {
  let view_id = select_default_published_view_id(pg_pool, workspace_id)
    .await?
    .ok_or_else(|| {
      AppError::RecordNotFound(format!(
        "Default published view not found for workspace_id: {}",
        workspace_id
      ))
    })?;

  let pub_info = select_published_collab_info(pg_pool, &view_id).await?;
  Ok(pub_info)
}

pub async fn get_workspace_default_publish_view_info_meta(
  pg_pool: &PgPool,
  namespace: &str,
) -> Result<(PublishInfo, serde_json::Value), AppError> {
  let view_id = select_default_published_view_id_for_namespace(pg_pool, namespace)
    .await?
    .ok_or_else(|| {
      AppError::RecordNotFound(format!(
        "Default published view not found for namespace: {}",
        namespace
      ))
    })?;

  let (pub_info, meta) = tokio::try_join!(
    select_published_collab_info(pg_pool, &view_id),
    select_published_metadata_for_view_id(pg_pool, &view_id)
  )?;
  let meta = meta.ok_or_else(|| {
    AppError::RecordNotFound(format!(
      "Published metadata not found for view_id: {}",
      view_id
    ))
  })?;

  Ok((pub_info, meta.1))
}

pub async fn get_workspace_publish_namespace(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<String, AppError> {
  let mut ws_namespaces = select_workspace_publish_namespaces(pg_pool, workspace_id).await?;
  match ws_namespaces.len() {
    0 => Err(AppError::RecordNotFound(format!(
      "No publish namespace found for workspace_id: {}",
      workspace_id
    ))),
    1 => Ok(ws_namespaces.remove(0).namespace),
    _ => {
      for ws_namespace in ws_namespaces {
        if !ws_namespace.is_original {
          return Ok(ws_namespace.namespace);
        }
      }
      Err(AppError::RecordNotFound(format!(
        "Cannot find non-original publish namespace for workspace_id: {}",
        workspace_id
      )))
    },
  }
}

pub async fn list_collab_publish_info(
  publish_collab_store: &dyn PublishedCollabStore,
  collab_instance_cache: &impl WorkspaceCollabInstanceCache,
  workspace_id: Uuid,
  uid: i64,
) -> Result<Vec<PublishInfoView>, AppError> {
  let folder = collab_instance_cache.get_folder(workspace_id).await?;
  let publish_infos = publish_collab_store
    .list_collab_publish_info(&workspace_id)
    .await?;

  let mut publish_info_views: Vec<PublishInfoView> = Vec::with_capacity(publish_infos.len());
  for publish_info in publish_infos {
    let view_id = publish_info.view_id.to_string();
    match folder.get_view(&view_id, uid) {
      Some(view) => {
        publish_info_views.push(PublishInfoView {
          view: to_dto_folder_view_miminal(&view),
          info: publish_info,
        });
      },
      None => {
        tracing::error!("View {} not found in folder but is published", view_id);
        publish_info_views.push(PublishInfoView {
          view: FolderViewMinimal {
            view_id,
            name: publish_info.publish_name.clone(),
            ..Default::default()
          },
          info: publish_info,
        });
      },
    };
  }

  Ok(publish_info_views)
}

async fn check_workspace_namespace(new_namespace: &str) -> Result<(), AppError> {
  // Must be url safe
  // Only contain alphanumeric characters and hyphens
  // and underscores (discouraged)
  for c in new_namespace.chars() {
    if !c.is_alphanumeric() && c != '-' && c != '_' {
      return Err(AppError::CustomNamespaceInvalidCharacter { character: c });
    }
  }
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

  async fn list_collab_publish_info(
    &self,
    workspace_id: &Uuid,
  ) -> Result<Vec<PublishInfo>, AppError>;

  async fn get_collab_publish_info(&self, view_id: &Uuid) -> Result<PublishInfo, AppError>;

  async fn get_collab_blob_by_publish_namespace(
    &self,
    publish_namespace: &str,
    publish_name: &str,
  ) -> Result<Vec<u8>, AppError>;

  async fn unpublish_collabs(
    &self,
    workspace_id: &Uuid,
    view_ids: &[Uuid],
    user_uuid: &Uuid,
  ) -> Result<(), AppError>;

  async fn patch_collabs(
    &self,
    workspace_id: &Uuid,
    user_uuid: &Uuid,
    patches: &[PatchPublishedCollab],
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
      check_view_id_publish_name_conflict(
        &self.pg_pool,
        workspace_id,
        &publish_item.meta.view_id,
        publish_item.meta.publish_name.as_str(),
      )
      .await?;
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

  async fn list_collab_publish_info(
    &self,
    workspace_id: &Uuid,
  ) -> Result<Vec<PublishInfo>, AppError> {
    select_all_published_collab_info(&self.pg_pool, workspace_id).await
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

  async fn unpublish_collabs(
    &self,
    workspace_id: &Uuid,
    view_ids: &[Uuid],
    user_uuid: &Uuid,
  ) -> Result<(), AppError> {
    check_workspace_owner_or_publisher(&self.pg_pool, user_uuid, workspace_id, view_ids).await?;
    set_published_collabs_as_unpublished(&self.pg_pool, workspace_id, view_ids).await?;
    Ok(())
  }

  async fn patch_collabs(
    &self,
    workspace_id: &Uuid,
    user_uuid: &Uuid,
    patches: &[PatchPublishedCollab],
  ) -> Result<(), AppError> {
    patch_collabs(&self.pg_pool, workspace_id, user_uuid, patches).await
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
      check_view_id_publish_name_conflict(
        &self.pg_pool,
        workspace_id,
        &publish_item.meta.view_id,
        publish_item.meta.publish_name.as_str(),
      )
      .await?;

      let object_key = get_collab_s3_key(workspace_id, &publish_item.meta.view_id);
      let data = publish_item.data.clone();
      let bucket_client = self.bucket_client.clone();
      let metrics = self.metrics.clone();
      let handle = tokio::spawn(async move {
        let body = ByteStream::from(data);
        let result = bucket_client.put_blob(&object_key, body, None).await;
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

  async fn list_collab_publish_info(
    &self,
    workspace_id: &Uuid,
  ) -> Result<Vec<PublishInfo>, AppError> {
    select_all_published_collab_info(&self.pg_pool, workspace_id).await
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

  async fn unpublish_collabs(
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
    set_published_collabs_as_unpublished(&self.pg_pool, workspace_id, view_ids).await?;
    Ok(())
  }

  async fn patch_collabs(
    &self,
    workspace_id: &Uuid,
    user_uuid: &Uuid,
    patches: &[PatchPublishedCollab],
  ) -> Result<(), AppError> {
    patch_collabs(&self.pg_pool, workspace_id, user_uuid, patches).await
  }
}

async fn patch_collabs(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  user_uuid: &Uuid,
  patches: &[PatchPublishedCollab],
) -> Result<(), AppError> {
  let view_ids = patches
    .iter()
    .map(|patch| patch.view_id)
    .collect::<Vec<Uuid>>();
  for patch in patches {
    if let Some(new_publish_name) = patch.publish_name.as_deref() {
      check_collab_publish_name(new_publish_name)?;
      check_publish_name_already_exists(pg_pool, workspace_id, new_publish_name).await?;
    }
  }
  check_workspace_owner_or_publisher(pg_pool, user_uuid, workspace_id, &view_ids).await?;

  let mut txn = pg_pool.begin().await?;
  update_published_collabs(&mut txn, workspace_id, patches).await?;
  txn.commit().await?;
  Ok(())
}

/// Checks if the `publish_name` already exists for the workspace
async fn check_publish_name_already_exists(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  publish_name: &str,
) -> Result<(), AppError> {
  let publish_name_exists = select_publish_name_exists(pg_pool, workspace_id, publish_name).await?;
  if publish_name_exists {
    return Err(AppError::PublishNameAlreadyExists {
      workspace_id: *workspace_id,
      publish_name: publish_name.to_string(),
    });
  }
  Ok(())
}

/// Check if the `publish_name` already exists on another view
async fn check_view_id_publish_name_conflict(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  view_id: &Uuid,
  publish_name: &str,
) -> Result<(), AppError> {
  match select_view_id_from_publish_name(pg_pool, workspace_id, publish_name).await? {
    Some(published_view_id) => {
      if published_view_id != *view_id {
        Err(AppError::PublishNameAlreadyExists {
          workspace_id: *workspace_id,
          publish_name: publish_name.to_string(),
        })
      } else {
        Ok(())
      }
    },
    None => Ok(()),
  }
}
