use crate::collab::{collab_db_ops, is_collab_exists};
use anyhow::{anyhow, Context};
use app_error::AppError;
use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use collab::core::collab_plugin::EncodedCollab;

use database_entity::dto::{
  AFAccessLevel, AFRole, AFSnapshotMeta, AFSnapshotMetas, CollabParams, CreateCollabParams,
  InsertSnapshotParams, QueryCollab, QueryCollabParams, QueryCollabResult, SnapshotData,
};

use sqlx::types::Uuid;
use sqlx::{PgPool, Transaction};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tracing::{debug, event, warn};
use validator::Validate;

pub const COLLAB_SNAPSHOT_LIMIT: i64 = 15;
pub const SNAPSHOT_PER_HOUR: i64 = 6;
pub type DatabaseResult<T, E = AppError> = core::result::Result<T, E>;

/// [CollabStorageAccessControl] is a trait that provides access control when accessing the storage
/// of the Collab object.
#[async_trait]
pub trait CollabStorageAccessControl: Send + Sync + 'static {
  /// Checks if the user with the given ID can access the [Collab] with the given ID.
  async fn get_collab_access_level(&self, uid: &i64, oid: &str) -> Result<AFAccessLevel, AppError>;

  /// Updates the cache of the access level of the user for given collab object.
  async fn cache_collab_access_level(
    &self,
    uid: &i64,
    oid: &str,
    level: AFAccessLevel,
  ) -> Result<(), AppError>;

  /// Returns the role of the user in the workspace.
  async fn get_user_workspace_role(
    &self,
    uid: &i64,
    workspace_id: &str,
  ) -> Result<AFRole, AppError>;
}

/// Represents a storage mechanism for collaborations.
///
/// This trait provides asynchronous methods for CRUD operations related to collaborations.
/// Implementors of this trait should provide the actual storage logic, be it in-memory, file-based, database-backed, etc.
#[async_trait]
pub trait CollabStorage: Send + Sync + 'static {
  fn config(&self) -> &WriteConfig;
  /// Checks if a collaboration with the given object ID exists in the storage.
  ///
  /// # Arguments
  ///
  /// * `object_id` - A string slice that holds the ID of the collaboration.
  ///
  /// # Returns
  ///
  /// * `bool` - `true` if the collaboration exists, `false` otherwise.
  async fn is_exist(&self, object_id: &str) -> bool;

  async fn cache_collab(&self, object_id: &str, collab: Weak<MutexCollab>);

  async fn remove_collab_cache(&self, object_id: &str);

  async fn is_collab_exist(&self, oid: &str) -> DatabaseResult<bool>;

  async fn upsert_collab(&self, uid: &i64, params: CreateCollabParams) -> DatabaseResult<()>;

  /// Insert/update a new collaboration in the storage.
  ///
  /// # Arguments
  ///
  /// * `params` - The parameters required to create a new collaboration.
  ///
  /// # Returns
  ///
  /// * `Result<()>` - Returns `Ok(())` if the collaboration was created successfully, `Err` otherwise.
  async fn upsert_collab_with_transaction(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> DatabaseResult<()>;

  /// Retrieves a collaboration from the storage.
  ///
  /// # Arguments
  ///
  /// * `params` - The parameters required to query a collab object.
  /// * `force_from_disk` - If `true`, the data will be retrieved from the disk instead of the cache.
  ///
  /// # Returns
  ///
  /// * `Result<RawData>` - Returns the data of the collaboration if found, `Err` otherwise.
  async fn get_collab_encoded(
    &self,
    uid: &i64,
    params: QueryCollabParams,
    force_from_disk: bool,
  ) -> DatabaseResult<EncodedCollab>;

  async fn batch_get_collab(
    &self,
    uid: &i64,
    queries: Vec<QueryCollab>,
  ) -> HashMap<String, QueryCollabResult>;

  /// Deletes a collaboration from the storage.
  ///
  /// # Arguments
  ///
  /// * `object_id` - A string slice that holds the ID of the collaboration to delete.
  ///
  /// # Returns
  ///
  /// * `Result<()>` - Returns `Ok(())` if the collaboration was deleted successfully, `Err` otherwise.
  async fn delete_collab(&self, uid: &i64, object_id: &str) -> DatabaseResult<()>;
  async fn should_create_snapshot(&self, oid: &str) -> bool;

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> DatabaseResult<AFSnapshotMeta>;

  async fn get_collab_snapshot(&self, snapshot_id: &i64) -> DatabaseResult<SnapshotData>;

  async fn get_collab_snapshot_list(&self, oid: &str) -> DatabaseResult<AFSnapshotMetas>;
}

#[async_trait]
impl<T> CollabStorage for Arc<T>
where
  T: CollabStorage,
{
  fn config(&self) -> &WriteConfig {
    self.as_ref().config()
  }

  async fn is_exist(&self, object_id: &str) -> bool {
    self.as_ref().is_exist(object_id).await
  }

  async fn cache_collab(&self, object_id: &str, collab: Weak<MutexCollab>) {
    self.as_ref().cache_collab(object_id, collab).await
  }

  async fn remove_collab_cache(&self, object_id: &str) {
    self.as_ref().remove_collab_cache(object_id).await
  }

  async fn is_collab_exist(&self, oid: &str) -> DatabaseResult<bool> {
    self.as_ref().is_collab_exist(oid).await
  }

  async fn upsert_collab(&self, uid: &i64, params: CreateCollabParams) -> DatabaseResult<()> {
    self.as_ref().upsert_collab(uid, params).await
  }

  async fn upsert_collab_with_transaction(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> DatabaseResult<()> {
    self
      .as_ref()
      .upsert_collab_with_transaction(workspace_id, uid, params, transaction)
      .await
  }

  async fn get_collab_encoded(
    &self,
    uid: &i64,
    params: QueryCollabParams,
    force_from_disk: bool,
  ) -> DatabaseResult<EncodedCollab> {
    self
      .as_ref()
      .get_collab_encoded(uid, params, force_from_disk)
      .await
  }

  async fn batch_get_collab(
    &self,
    uid: &i64,
    queries: Vec<QueryCollab>,
  ) -> HashMap<String, QueryCollabResult> {
    self.as_ref().batch_get_collab(uid, queries).await
  }

  async fn delete_collab(&self, uid: &i64, object_id: &str) -> DatabaseResult<()> {
    self.as_ref().delete_collab(uid, object_id).await
  }

  async fn should_create_snapshot(&self, oid: &str) -> bool {
    self.as_ref().should_create_snapshot(oid).await
  }

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> DatabaseResult<AFSnapshotMeta> {
    self.as_ref().create_snapshot(params).await
  }

  async fn get_collab_snapshot(&self, snapshot_id: &i64) -> DatabaseResult<SnapshotData> {
    self.as_ref().get_collab_snapshot(snapshot_id).await
  }

  async fn get_collab_snapshot_list(&self, oid: &str) -> DatabaseResult<AFSnapshotMetas> {
    self.as_ref().get_collab_snapshot_list(oid).await
  }
}

#[derive(Debug, Clone)]
pub struct WriteConfig {
  pub flush_per_update: u32,
}

impl Default for WriteConfig {
  fn default() -> Self {
    Self {
      flush_per_update: 100,
    }
  }
}

#[derive(Clone)]
pub struct CollabStoragePgImpl {
  pg_pool: PgPool,
  config: WriteConfig,
}

impl CollabStoragePgImpl {
  pub fn new(pg_pool: PgPool) -> Self {
    let config = WriteConfig::default();
    Self { pg_pool, config }
  }
}

#[async_trait]
impl CollabStorage for CollabStoragePgImpl {
  fn config(&self) -> &WriteConfig {
    &self.config
  }

  async fn is_exist(&self, object_id: &str) -> bool {
    collab_db_ops::collab_exists(&self.pg_pool, object_id)
      .await
      .unwrap_or(false)
  }

  async fn cache_collab(&self, _object_id: &str, _collab: Weak<MutexCollab>) {}

  async fn remove_collab_cache(&self, _object_id: &str) {}

  async fn is_collab_exist(&self, oid: &str) -> DatabaseResult<bool> {
    let is_exist = is_collab_exists(oid, &self.pg_pool).await?;
    Ok(is_exist)
  }

  async fn upsert_collab(&self, uid: &i64, params: CreateCollabParams) -> DatabaseResult<()> {
    let mut transaction = self
      .pg_pool
      .begin()
      .await
      .context("Failed to acquire a Postgres transaction to insert collab")?;
    let (params, workspace_id) = params.split();
    self
      .upsert_collab_with_transaction(&workspace_id, uid, params, &mut transaction)
      .await?;
    transaction
      .commit()
      .await
      .context("Failed to commit transaction to insert collab")?;
    Ok(())
  }

  async fn upsert_collab_with_transaction(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> DatabaseResult<()> {
    collab_db_ops::insert_into_af_collab(transaction, uid, workspace_id, &params).await?;
    Ok(())
  }

  async fn get_collab_encoded(
    &self,
    _uid: &i64,
    params: QueryCollabParams,
    _force_from_disk: bool,
  ) -> DatabaseResult<EncodedCollab> {
    event!(
      tracing::Level::DEBUG,
      "Get collab data:{} from disk",
      params.object_id
    );
    match collab_db_ops::select_blob_from_af_collab(
      &self.pg_pool,
      &params.collab_type,
      &params.object_id,
    )
    .await
    {
      Ok(data) => EncodedCollab::decode_from_bytes(&data)
        .map_err(|err| AppError::Internal(anyhow!("fail to decode data to EncodedDoc: {:?}", err))),
      Err(e) => match e {
        sqlx::Error::RowNotFound => {
          let msg = format!("Can't find the row for query: {:?}", params);
          Err(AppError::RecordNotFound(msg))
        },
        _ => Err(e.into()),
      },
    }
  }

  async fn batch_get_collab(
    &self,
    _uid: &i64,
    queries: Vec<QueryCollab>,
  ) -> HashMap<String, QueryCollabResult> {
    collab_db_ops::batch_select_collab_blob(&self.pg_pool, queries).await
  }

  async fn delete_collab(&self, _uid: &i64, object_id: &str) -> DatabaseResult<()> {
    collab_db_ops::delete_collab(&self.pg_pool, object_id).await?;
    Ok(())
  }

  async fn should_create_snapshot(&self, oid: &str) -> bool {
    if oid.is_empty() {
      warn!("unexpected empty object id when checking should_create_snapshot");
      return false;
    }

    collab_db_ops::should_create_snapshot(oid, &self.pg_pool)
      .await
      .unwrap_or(false)
  }

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> DatabaseResult<AFSnapshotMeta> {
    params.validate()?;

    debug!("create snapshot for object:{}", params.object_id);
    let meta = collab_db_ops::create_snapshot_and_maintain_limit(
      &self.pg_pool,
      &params.object_id,
      &params.encoded_collab_v1,
      &params.workspace_id.parse::<Uuid>()?,
      COLLAB_SNAPSHOT_LIMIT,
    )
    .await?;
    Ok(meta)
  }

  async fn get_collab_snapshot(&self, snapshot_id: &i64) -> DatabaseResult<SnapshotData> {
    match collab_db_ops::select_snapshot(&self.pg_pool, snapshot_id).await? {
      None => Err(AppError::RecordNotFound(format!(
        "Can't find the snapshot with id:{}",
        snapshot_id
      ))),
      Some(row) => Ok(SnapshotData {
        object_id: row.oid,
        encoded_collab_v1: row.blob,
        workspace_id: row.workspace_id.to_string(),
      }),
    }
  }

  async fn get_collab_snapshot_list(&self, oid: &str) -> DatabaseResult<AFSnapshotMetas> {
    let metas = collab_db_ops::get_all_collab_snapshot_meta(&self.pg_pool, oid).await?;
    Ok(metas)
  }
}
