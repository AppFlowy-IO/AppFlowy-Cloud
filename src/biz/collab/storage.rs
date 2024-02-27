use crate::biz::casbin::{CollabAccessControlImpl, WorkspaceAccessControlImpl};
use crate::biz::collab::access_control::CollabStorageAccessControlImpl;
use crate::biz::collab::mem_cache::CollabMemCache;
use crate::state::RedisClient;
use anyhow::Context;
use app_error::AppError;
use async_trait::async_trait;

use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;

use database::collab::{
  is_collab_exists, CollabStorage, CollabStorageAccessControl, CollabStoragePgImpl, DatabaseResult,
  WriteConfig,
};
use database_entity::dto::{
  AFAccessLevel, AFSnapshotMeta, AFSnapshotMetas, CollabParams, CreateCollabParams,
  InsertSnapshotParams, QueryCollab, QueryCollabParams, QueryCollabResult, SnapshotData,
};
use futures::stream::{self, StreamExt};
use itertools::{Either, Itertools};
use sqlx::{PgPool, Transaction};
use std::ops::DerefMut;
use std::time::Duration;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::oneshot;
use tokio::time::timeout;

use crate::biz::collab::metrics::CollabMetrics;
use crate::biz::snapshot::SnapshotControl;
use realtime::collaborate::{RTCommand, RTCommandSender};
use tracing::{error, event, instrument, Level};
use validator::Validate;

pub type CollabStorageImpl = CollabStoragePostgresImpl<
  CollabStorageAccessControlImpl<CollabAccessControlImpl, WorkspaceAccessControlImpl>,
>;

pub async fn init_collab_storage(
  pg_pool: PgPool,
  redis_client: RedisClient,
  collab_access_control: CollabAccessControlImpl,
  workspace_access_control: WorkspaceAccessControlImpl,
  collab_metrics: Arc<CollabMetrics>,
  realtime_server_command_sender: RTCommandSender,
) -> CollabStorageImpl {
  let access_control = CollabStorageAccessControlImpl {
    collab_access_control: collab_access_control.into(),
    workspace_access_control: workspace_access_control.into(),
  };
  let disk_cache = CollabStoragePgImpl::new(pg_pool.clone());
  let mem_cache = CollabMemCache::new(redis_client.clone());
  let snapshot_control = SnapshotControl::new(redis_client, pg_pool, collab_metrics).await;
  CollabStoragePostgresImpl::new(
    disk_cache,
    mem_cache,
    access_control,
    snapshot_control,
    realtime_server_command_sender,
  )
}

/// A wrapper around the actual storage implementation that provides access control and caching.
#[derive(Clone)]
pub struct CollabStoragePostgresImpl<AC> {
  disk_cache: CollabStoragePgImpl,
  mem_cache: CollabMemCache,
  /// access control for collab object. Including read/write
  access_control: AC,
  snapshot_control: SnapshotControl,
  rt_cmd: RTCommandSender,
}

impl<AC> CollabStoragePostgresImpl<AC>
where
  AC: CollabStorageAccessControl,
{
  pub fn new(
    disk_cache: CollabStoragePgImpl,
    mem_cache: CollabMemCache,
    access_control: AC,
    snapshot_control: SnapshotControl,
    rt_cmd_sender: RTCommandSender,
  ) -> Self {
    Self {
      disk_cache,
      mem_cache,
      access_control,
      snapshot_control,
      rt_cmd: rt_cmd_sender,
    }
  }

  async fn check_collab_permission(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: &CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> Result<(), AppError> {
    // Check if the user has enough permissions to insert collab
    // 1. If the collab already exists, check if the user has enough permissions to update collab
    // 2. If the collab doesn't exist, check if the user has enough permissions to create collab.
    let collab_exists = is_collab_exists(&params.object_id, transaction.deref_mut()).await?;
    if collab_exists {
      // If the collab already exists, check if the user has enough permissions to update collab
      let can_write = self
        .access_control
        .get_or_refresh_collab_access_level(uid, &params.object_id, transaction.deref_mut())
        .await
        .context(format!(
          "Can't find the access level when user:{} try to insert collab",
          uid
        ))?
        .can_write();
      if !can_write {
        return Err(AppError::NotEnoughPermissions(format!(
          "user:{} doesn't have enough permissions to update collab {}",
          uid, params.object_id
        )));
      }
    } else {
      // If the collab doesn't exist, check if the user has enough permissions to create collab.
      // If the user is the owner or member of the workspace, the user can create collab.
      let can_write_workspace = self
        .access_control
        .get_user_workspace_role(uid, workspace_id, transaction.deref_mut())
        .await?
        .can_create_collab();

      if !can_write_workspace {
        return Err(AppError::NotEnoughPermissions(format!(
          "user:{} doesn't have enough permissions to insert collab {}",
          uid, params.object_id
        )));
      }

      // Cache the access level if the user has enough permissions to create collab.
      self
        .access_control
        .cache_collab_access_level(uid, &params.object_id, AFAccessLevel::FullAccess)
        .await?;
    }

    Ok(())
  }

  async fn get_encode_collab_from_editing(&self, object_id: &str) -> Option<EncodedCollab> {
    let object_id = object_id.to_string();
    let (ret, rx) = oneshot::channel();
    let timeout_duration = Duration::from_secs(5);

    // Attempt to send the command to the realtime server
    if let Err(err) = self
      .rt_cmd
      .send(RTCommand::GetEncodeCollab { object_id, ret })
      .await
    {
      error!(
        "Failed to send get encode collab command to realtime server: {}",
        err
      );
      return None;
    }

    // Await the response from the realtime server with a timeout
    match timeout(timeout_duration, rx).await {
      Ok(Ok(Some(encode_collab))) => Some(encode_collab),
      Ok(Ok(None)) => None,
      Ok(Err(err)) => {
        error!("Failed to get encode collab from realtime server: {}", err);
        None
      },
      Err(_) => {
        error!("Timeout waiting for encode collab from realtime server");
        None
      },
    }
  }
}

#[async_trait]
impl<AC> CollabStorage for CollabStoragePostgresImpl<AC>
where
  AC: CollabStorageAccessControl,
{
  fn config(&self) -> &WriteConfig {
    self.disk_cache.config()
  }

  fn encode_collab_mem_hit_rate(&self) -> f64 {
    self.mem_cache.get_hit_rate()
  }

  async fn upsert_collab(&self, uid: &i64, params: CreateCollabParams) -> DatabaseResult<()> {
    let mut transaction = self
      .disk_cache
      .pg_pool
      .begin()
      .await
      .context("acquire transaction to upsert collab")
      .map_err(AppError::from)?;
    let (params, workspace_id) = params.split();
    self
      .upsert_collab_with_transaction(&workspace_id, uid, params, &mut transaction)
      .await?;
    transaction
      .commit()
      .await
      .context("fail to commit the transaction to upsert collab")
      .map_err(AppError::from)?;
    Ok(())
  }

  #[instrument(level = "trace", skip(self, params), oid = %params.oid, err)]
  #[allow(clippy::blocks_in_if_conditions)]
  async fn upsert_collab_with_transaction(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> DatabaseResult<()> {
    params.validate()?;
    self
      .check_collab_permission(workspace_id, uid, &params, transaction)
      .await?;

    // Check if the data can be decoded into collab
    if let Err(err) = check_encoded_collab_data(&params.object_id, &params.encoded_collab_v1) {
      let msg = format!(
        "Can not decode the data into collab:{}, {}",
        params.object_id, err
      );
      return Err(AppError::InvalidRequest(msg));
    }

    let object_id = params.object_id.clone();
    let encoded_collab = params.encoded_collab_v1.clone();
    self
      .disk_cache
      .upsert_collab_with_transaction(workspace_id, uid, params, transaction)
      .await?;

    self
      .mem_cache
      .insert_encode_collab_bytes(object_id, encoded_collab)
      .await;
    Ok(())
  }

  async fn get_collab_encoded(
    &self,
    uid: &i64,
    params: QueryCollabParams,
    is_collab_init: bool,
  ) -> DatabaseResult<EncodedCollab> {
    params.validate()?;
    self
      .access_control
      .get_or_refresh_collab_access_level(uid, &params.object_id, &self.disk_cache.pg_pool)
      .await?;

    // Early return if editing collab is initialized, as it indicates no need to query further.
    if !is_collab_init {
      // Attempt to retrieve encoded collab from the editing collab
      if let Some(value) = self.get_encode_collab_from_editing(&params.object_id).await {
        return Ok(value);
      }
    }

    // Attempt to retrieve encoded collab from memory cache, falling back to disk cache if necessary.
    if let Some(encoded_collab) = self
      .mem_cache
      .get_encode_collab(&params.inner.object_id)
      .await
    {
      event!(
        Level::DEBUG,
        "Get encoded collab:{} from cache",
        params.object_id
      );
      return Ok(encoded_collab);
    }

    // Retrieve from disk cache as fallback. After retrieval, the value is inserted into the memory cache.
    let object_id = params.object_id.clone();
    let encoded_collab = self.disk_cache.get_collab_encoded(uid, params).await?;
    self
      .mem_cache
      .insert_encode_collab(object_id, &encoded_collab)
      .await;
    Ok(encoded_collab)
  }

  async fn batch_get_collab(
    &self,
    uid: &i64,
    queries: Vec<QueryCollab>,
  ) -> HashMap<String, QueryCollabResult> {
    // 1. Partition queries based on validation into valid queries and errors (with associated error messages).
    let (valid_queries, mut results): (Vec<_>, HashMap<_, _>) =
      queries
        .into_iter()
        .partition_map(|params| match params.validate() {
          Ok(_) => Either::Left(params),
          Err(err) => Either::Right((
            params.object_id,
            QueryCollabResult::Failed {
              error: err.to_string(),
            },
          )),
        });

    // 2. Processes valid queries against the in-memory cache to retrieve cached values.
    //    - Queries not found in the cache are earmarked for disk retrieval.
    let (disk_queries, values_from_mem_cache): (Vec<_>, HashMap<_, _>) =
      stream::iter(valid_queries)
        .then(|params| async move {
          match self
            .mem_cache
            .get_encode_collab_bytes(&params.object_id)
            .await
          {
            None => Either::Left(params),
            Some(data) => Either::Right((
              params.object_id.clone(),
              QueryCollabResult::Success {
                encode_collab_v1: data,
              },
            )),
          }
        })
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .partition_map(|either| either);
    results.extend(values_from_mem_cache);

    // 3. Retrieves remaining values from the disk cache for queries not satisfied by the memory cache.
    //    - These values are then merged into the final result set.
    let values_from_disk_cache = self.disk_cache.batch_get_collab(uid, disk_queries).await;
    results.extend(values_from_disk_cache);

    results
  }

  async fn delete_collab(&self, uid: &i64, object_id: &str) -> DatabaseResult<()> {
    if !self
      .access_control
      .get_or_refresh_collab_access_level(uid, object_id, &self.disk_cache.pg_pool)
      .await
      .context(format!(
        "Can't find the access level when user:{} try to delete {}",
        uid, object_id
      ))?
      .can_delete()
    {
      return Err(AppError::NotEnoughPermissions(format!(
        "user:{} doesn't have enough permissions to delete collab {}",
        uid, object_id
      )));
    }
    self.mem_cache.remove_encode_collab(object_id).await;
    self.disk_cache.delete_collab(uid, object_id).await
  }

  async fn should_create_snapshot(&self, oid: &str) -> bool {
    self.disk_cache.should_create_snapshot(oid).await
  }

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> DatabaseResult<AFSnapshotMeta> {
    self.disk_cache.create_snapshot(params).await
  }

  async fn queue_snapshot(&self, params: InsertSnapshotParams) -> DatabaseResult<()> {
    self.snapshot_control.queue_snapshot(params).await
  }

  async fn get_collab_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    snapshot_id: &i64,
  ) -> DatabaseResult<SnapshotData> {
    match self
      .snapshot_control
      .get_snapshot(workspace_id, object_id)
      .await
    {
      None => self.disk_cache.get_collab_snapshot(snapshot_id).await,
      Some(data) => Ok(data),
    }
  }

  async fn get_collab_snapshot_list(&self, oid: &str) -> DatabaseResult<AFSnapshotMetas> {
    self.disk_cache.get_collab_snapshot_list(oid).await
  }
}

pub fn check_encoded_collab_data(object_id: &str, data: &[u8]) -> Result<(), anyhow::Error> {
  let encoded_collab = EncodedCollab::decode_from_bytes(data)?;
  let _ = Collab::new_with_doc_state(
    CollabOrigin::Empty,
    object_id,
    encoded_collab.doc_state.to_vec(),
    vec![],
  )?;
  Ok(())
}
