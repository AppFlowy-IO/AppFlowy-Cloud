use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use database::collab::{
  CollabStorage, CollabStorageAccessControl, CollabStoragePgImpl, DatabaseResult, StorageConfig,
};
use database_entity::dto::{
  AFCollabSnapshots, BatchQueryCollab, InsertCollabParams, InsertSnapshotParams, QueryCollabParams,
  QueryCollabResult, QueryObjectSnapshotParams, QuerySnapshotParams, RawData,
};
use itertools::{Either, Itertools};

use crate::biz::collab::access_control::{CollabAccessControlImpl, CollabStorageAccessControlImpl};
use crate::biz::workspace::access_control::WorkspaceAccessControlImpl;
use anyhow::Context;
use database_entity::error::DatabaseError;
use sqlx::PgPool;
use std::{
  collections::HashMap,
  sync::{Arc, Weak},
};
use tokio::sync::RwLock;
use tracing::{event, info, instrument};
use validator::Validate;

pub type CollabPostgresDBStorage = CollabStorageWrapper<
  CollabStorageAccessControlImpl<CollabAccessControlImpl, WorkspaceAccessControlImpl>,
>;

pub async fn init_collab_storage(
  pg_pool: PgPool,
  collab_access_control: Arc<CollabAccessControlImpl>,
  workspace_access_control: Arc<WorkspaceAccessControlImpl>,
) -> CollabPostgresDBStorage {
  let access_control = CollabStorageAccessControlImpl {
    collab_access_control,
    workspace_access_control,
  };
  let collab_storage_impl = CollabStoragePgImpl::new(pg_pool);
  CollabStorageWrapper::new(collab_storage_impl, access_control)
}

/// A wrapper around the actual storage implementation that provides access control and caching.
#[derive(Clone)]
pub struct CollabStorageWrapper<AC> {
  inner: CollabStoragePgImpl,
  access_control: AC,
  collab_by_object_id: Arc<RwLock<HashMap<String, Weak<MutexCollab>>>>,
}

impl<AC> CollabStorageWrapper<AC>
where
  AC: CollabStorageAccessControl,
{
  pub fn new(inner: CollabStoragePgImpl, access_control: AC) -> Self {
    Self {
      inner,
      access_control,
      collab_by_object_id: Arc::new(RwLock::new(HashMap::new())),
    }
  }
}

#[async_trait]
impl<AC> CollabStorage for CollabStorageWrapper<AC>
where
  AC: CollabStorageAccessControl,
{
  fn config(&self) -> &StorageConfig {
    self.inner.config()
  }

  async fn is_exist(&self, object_id: &str) -> bool {
    self.inner.is_exist(object_id).await
  }

  async fn cache_collab(&self, object_id: &str, collab: Weak<MutexCollab>) {
    tracing::trace!("Cache collab:{} in memory", object_id);
    self
      .collab_by_object_id
      .write()
      .await
      .insert(object_id.to_string(), collab);
  }

  async fn is_collab_exist(&self, oid: &str) -> DatabaseResult<bool> {
    self.inner.is_collab_exist(oid).await
  }

  #[instrument(level = "trace", skip(self, params), oid = %params.oid, err)]
  async fn insert_collab(&self, uid: &i64, params: InsertCollabParams) -> DatabaseResult<()> {
    params.validate()?;

    // Check if the user has enough permissions to insert collab
    let has_permission = if self.is_collab_exist(&params.object_id).await? {
      // If the collab already exists, check if the user has enough permissions to update collab
      let level = self
        .access_control
        .get_collab_access_level(uid, &params.object_id)
        .await
        .context(format!(
          "Can't find the access level when user:{} try to insert collab",
          uid
        ))?;
      event!(
        tracing::Level::TRACE,
        "user:{} with {:?} try to update exist collab:{}",
        uid,
        level,
        params.object_id
      );

      level.can_write()
    } else {
      // If the collab doesn't exist, check if the user has enough permissions to create collab.
      // If the user is the owner or member of the workspace, the user can create collab.
      let role = self
        .access_control
        .get_user_role(uid, &params.workspace_id)
        .await?;
      event!(
        tracing::Level::TRACE,
        "[{:?}]user:{} try to insert new collab:{}",
        role,
        uid,
        params.object_id
      );
      role.can_create_collab()
    };

    if !has_permission {
      return Err(DatabaseError::NotEnoughPermissions(format!(
        "user:{} doesn't have enough permissions to insert collab {}",
        uid, params.object_id
      )));
    }
    self.inner.insert_collab(uid, params).await
  }

  async fn get_collab(&self, uid: &i64, params: QueryCollabParams) -> DatabaseResult<RawData> {
    params.validate()?;
    let _ = self
      .access_control
      .get_collab_access_level(uid, &params.object_id)
      .await?;

    let collab = self
      .collab_by_object_id
      .read()
      .await
      .get(&params.object_id)
      .and_then(|collab| collab.upgrade());

    match collab {
      None => self.inner.get_collab(uid, params).await,
      Some(collab) => {
        info!("Get collab data:{} from memory", params.object_id);
        let data = collab.encode_as_update_v1().0;
        Ok(data)
      },
    }
  }

  async fn batch_get_collab(
    &self,
    uid: &i64,
    queries: Vec<BatchQueryCollab>,
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

    let read_guard = self.collab_by_object_id.read().await;
    let (results_from_memory, queries): (HashMap<_, _>, Vec<_>) =
      valid_queries.into_iter().partition_map(|params| {
        match read_guard
          .get(&params.object_id)
          .and_then(|collab| collab.upgrade())
        {
          Some(collab) => Either::Left((
            params.object_id,
            QueryCollabResult::Success {
              blob: collab.encode_as_update_v1().0,
            },
          )),
          None => Either::Right(params),
        }
      });

    results.extend(results_from_memory);
    results.extend(self.inner.batch_get_collab(uid, queries).await);
    results
  }

  async fn delete_collab(&self, uid: &i64, object_id: &str) -> DatabaseResult<()> {
    if !self
      .access_control
      .get_collab_access_level(uid, object_id)
      .await
      .context(format!(
        "Can't find the access level when user:{} try to delete {}",
        uid, object_id
      ))?
      .can_delete()
    {
      return Err(DatabaseError::NotEnoughPermissions(format!(
        "user:{} doesn't have enough permissions to delete collab {}",
        uid, object_id
      )));
    }
    self.inner.delete_collab(uid, object_id).await
  }

  async fn create_snapshot(
    &self,
    params: InsertSnapshotParams,
  ) -> database::collab::DatabaseResult<()> {
    self.inner.create_snapshot(params).await
  }

  async fn get_snapshot_data(
    &self,
    params: QuerySnapshotParams,
  ) -> database::collab::DatabaseResult<RawData> {
    self.inner.get_snapshot_data(params).await
  }

  async fn get_all_snapshots(
    &self,
    params: QueryObjectSnapshotParams,
  ) -> database::collab::DatabaseResult<AFCollabSnapshots> {
    self.inner.get_all_snapshots(params).await
  }
}
