use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use database::collab::{
  is_collab_exists, CollabStorage, CollabStorageAccessControl, CollabStoragePgImpl, DatabaseResult,
  WriteConfig,
};
use database_entity::dto::{
  AFAccessLevel, AFSnapshotMeta, AFSnapshotMetas, CollabParams, CreateCollabParams,
  InsertSnapshotParams, QueryCollab, QueryCollabParams, QueryCollabResult, SnapshotData,
};
use itertools::{Either, Itertools};

use crate::biz::casbin::{CollabAccessControlImpl, WorkspaceAccessControlImpl};
use crate::biz::collab::access_control::CollabStorageAccessControlImpl;
use crate::biz::collab::mem_cache::CollabMemCache;
use crate::state::RedisClient;
use anyhow::Context;
use app_error::AppError;
use collab::core::collab_plugin::EncodedCollab;
use dashmap::DashMap;
use sqlx::{PgPool, Transaction};
use std::ops::DerefMut;
use std::{
  collections::HashMap,
  sync::{Arc, Weak},
};

use tracing::{event, instrument};
use validator::Validate;

pub type CollabPostgresDBStorage = CollabStorageController<
  CollabStorageAccessControlImpl<CollabAccessControlImpl, WorkspaceAccessControlImpl>,
>;

pub async fn init_collab_storage(
  pg_pool: PgPool,
  redis_client: RedisClient,
  collab_access_control: CollabAccessControlImpl,
  workspace_access_control: WorkspaceAccessControlImpl,
) -> CollabPostgresDBStorage {
  let access_control = CollabStorageAccessControlImpl {
    collab_access_control: collab_access_control.into(),
    workspace_access_control: workspace_access_control.into(),
  };
  let disk_cache = CollabStoragePgImpl::new(pg_pool);
  let mem_cache = CollabMemCache::new(redis_client);
  CollabStorageController::new(disk_cache, mem_cache, access_control)
}

/// A wrapper around the actual storage implementation that provides access control and caching.
#[derive(Clone)]
pub struct CollabStorageController<AC> {
  disk_cache: CollabStoragePgImpl,
  mem_cache: CollabMemCache,
  /// access control for collab object. Including read/write
  access_control: AC,
  /// cache opened collab by object_id. The collab will be removed from the cache when it's closed.
  opened_collab_by_object_id: Arc<DashMap<String, Weak<MutexCollab>>>,
}

impl<AC> CollabStorageController<AC>
where
  AC: CollabStorageAccessControl,
{
  pub fn new(
    disk_cache: CollabStoragePgImpl,
    mem_cache: CollabMemCache,
    access_control: AC,
  ) -> Self {
    Self {
      disk_cache,
      mem_cache,
      access_control,
      opened_collab_by_object_id: Arc::new(DashMap::new()),
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
}

#[async_trait]
impl<AC> CollabStorage for CollabStorageController<AC>
where
  AC: CollabStorageAccessControl,
{
  fn config(&self) -> &WriteConfig {
    self.disk_cache.config()
  }

  fn encode_collab_mem_hit_rate(&self) -> f64 {
    self.mem_cache.get_hit_rate()
  }

  async fn cache_collab(&self, object_id: &str, collab: Weak<MutexCollab>) {
    tracing::trace!("cache opened collab:{}", object_id);
    self
      .opened_collab_by_object_id
      .insert(object_id.to_string(), collab);
  }

  async fn remove_collab_cache(&self, object_id: &str) {
    tracing::trace!("remove opened collab:{} cache", object_id);
    self.opened_collab_by_object_id.remove(object_id);
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
    let object_id = params.object_id.clone();
    let encoded_collab = params.encoded_collab_v1.clone();
    self
      .disk_cache
      .upsert_collab_with_transaction(workspace_id, uid, params, transaction)
      .await?;

    self
      .mem_cache
      .cache_encoded_collab_bytes(object_id, encoded_collab)
      .await;
    Ok(())
  }

  async fn get_collab_encoded(
    &self,
    uid: &i64,
    params: QueryCollabParams,
  ) -> DatabaseResult<EncodedCollab> {
    params.validate()?;
    self
      .access_control
      .get_or_refresh_collab_access_level(uid, &params.object_id, &self.disk_cache.pg_pool)
      .await?;
    let object_id = params.object_id.clone();

    // Attempt to retrieve from the opened collab cache
    if let Some(collab_weak_ref) = self
      .opened_collab_by_object_id
      .get(&params.object_id)
      .and_then(|entry| entry.value().upgrade())
    {
      event!(
        tracing::Level::DEBUG,
        "Get encoded collab:{} from memory",
        params.object_id
      );
      let data = collab_weak_ref.encode_collab_v1();
      return Ok(data);
    }

    // Attempt to retrieve from memory cache if not found then try
    // to retrieve from disk cache
    match self
      .mem_cache
      .get_encoded_collab(&params.inner.object_id)
      .await
    {
      Some(encoded_collab) => {
        event!(
          tracing::Level::DEBUG,
          "Get encoded collab:{} from cache",
          params.object_id
        );
        Ok(encoded_collab)
      },
      None => {
        // Fallback to disk cache if not in memory cache
        let encoded_collab = self.disk_cache.get_collab_encoded(uid, params).await?;
        self
          .mem_cache
          .cache_encoded_collab(object_id, &encoded_collab)
          .await;
        Ok(encoded_collab)
      },
    }
  }

  async fn batch_get_collab(
    &self,
    uid: &i64,
    queries: Vec<QueryCollab>,
  ) -> HashMap<String, QueryCollabResult> {
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

    let (results_from_memory, queries): (HashMap<_, _>, Vec<_>) =
      valid_queries.into_iter().partition_map(|params| {
        match self
          .opened_collab_by_object_id
          .get(&params.object_id)
          .and_then(|entry| entry.value().upgrade())
        {
          Some(collab) => match collab.encode_collab_v1().encode_to_bytes() {
            Ok(bytes) => Either::Left((
              params.object_id,
              QueryCollabResult::Success {
                encode_collab_v1: bytes,
              },
            )),
            Err(_) => Either::Right(params),
          },
          None => Either::Right(params),
        }
      });

    results.extend(results_from_memory);
    results.extend(self.disk_cache.batch_get_collab(uid, queries).await);
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
    self.disk_cache.delete_collab(uid, object_id).await
  }

  async fn should_create_snapshot(&self, oid: &str) -> bool {
    self.disk_cache.should_create_snapshot(oid).await
  }

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> DatabaseResult<AFSnapshotMeta> {
    self.disk_cache.create_snapshot(params).await
  }

  async fn get_collab_snapshot(&self, snapshot_id: &i64) -> DatabaseResult<SnapshotData> {
    self.disk_cache.get_collab_snapshot(snapshot_id).await
  }

  async fn get_collab_snapshot_list(&self, oid: &str) -> DatabaseResult<AFSnapshotMetas> {
    self.disk_cache.get_collab_snapshot_list(oid).await
  }
}
