use anyhow::Context;
use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use collab_define::CollabType;
use database_entity::error::DatabaseError;
use database_entity::{AFCollabSnapshots, QueryObjectSnapshotParams, QuerySnapshotParams};
use redis::aio::ConnectionManager;
use shared_entity::dto::{InsertCollabParams, InsertSnapshotParams, QueryCollabParams};
use sqlx::types::Uuid;
use sqlx::PgPool;
use std::sync::Weak;
use validator::Validate;

use crate::collaborate;
type RawData = Vec<u8>;

pub type Result<T, E = DatabaseError> = core::result::Result<T, E>;

/// Represents a storage mechanism for collaborations.
///
/// This trait provides asynchronous methods for CRUD operations related to collaborations.
/// Implementors of this trait should provide the actual storage logic, be it in-memory, file-based, database-backed, etc.
#[async_trait]
pub trait CollabStorage: Clone + Send + Sync + 'static {
  fn config(&self) -> &StorageConfig;
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

  async fn cache_collab(&self, _object_id: &str, _collab: Weak<MutexCollab>) {}

  /// Creates a new collaboration in the storage.
  ///
  /// # Arguments
  ///
  /// * `params` - The parameters required to create a new collaboration.
  ///
  /// # Returns
  ///
  /// * `Result<()>` - Returns `Ok(())` if the collaboration was created successfully, `Err` otherwise.
  async fn insert_collab(&self, owner_uid: i64, params: InsertCollabParams) -> Result<()>;

  /// Retrieves a collaboration from the storage.
  ///
  /// # Arguments
  ///
  /// * `params` - The parameters required to query a collab object.
  ///
  /// # Returns
  ///
  /// * `Result<RawData>` - Returns the data of the collaboration if found, `Err` otherwise.
  async fn get_collab(&self, params: QueryCollabParams) -> Result<RawData>;

  /// Deletes a collaboration from the storage.
  ///
  /// # Arguments
  ///
  /// * `object_id` - A string slice that holds the ID of the collaboration to delete.
  ///
  /// # Returns
  ///
  /// * `Result<()>` - Returns `Ok(())` if the collaboration was deleted successfully, `Err` otherwise.
  async fn delete_collab(&self, object_id: &str) -> Result<()>;

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> Result<()>;

  async fn get_snapshot_data(&self, params: QuerySnapshotParams) -> Result<RawData>;

  async fn get_all_snapshots(&self, params: QueryObjectSnapshotParams)
    -> Result<AFCollabSnapshots>;
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
  pub flush_per_update: u32,
}

impl Default for StorageConfig {
  fn default() -> Self {
    Self {
      flush_per_update: FLUSH_PER_UPDATE,
    }
  }
}

#[derive(Clone)]
pub struct CollabDatabaseStorageImpl {
  #[allow(dead_code)]
  pg_pool: PgPool,
  redis_client: ConnectionManager,
  config: StorageConfig,
}

pub const FLUSH_PER_UPDATE: u32 = 100;
impl CollabDatabaseStorageImpl {
  pub fn new(pg_pool: PgPool, redis_client: ConnectionManager) -> Self {
    let config = StorageConfig {
      flush_per_update: FLUSH_PER_UPDATE,
    };
    Self {
      pg_pool,
      redis_client,
      config,
    }
  }
}

#[async_trait]
impl CollabStorage for CollabDatabaseStorageImpl {
  fn config(&self) -> &StorageConfig {
    &self.config
  }

  async fn is_exist(&self, object_id: &str) -> bool {
    collaborate::collab_exists(&self.pg_pool, object_id)
      .await
      .unwrap_or(false)
  }

  async fn insert_collab(&self, owner_uid: i64, params: InsertCollabParams) -> Result<()> {
    params.validate()?;
    let mut transaction = self.pg_pool.begin().await?;
    collaborate::insert_af_collab(
      &mut transaction,
      self.redis_client.clone(),
      &params
        .workspace_id
        .parse::<Uuid>()
        .context("Failed to parse workspace ID")?,
      owner_uid,
      &params.object_id,
      &params.raw_data,
      &params.collab_type,
    )
    .await?;
    transaction.commit().await?;
    Ok(())
  }

  async fn get_collab(&self, params: QueryCollabParams) -> Result<RawData> {
    params.validate()?;

    match collaborate::get_collab_blob_cached(
      &self.pg_pool,
      self.redis_client.clone(),
      &params.collab_type,
      &params.object_id,
    )
    .await
    {
      Ok(data) => {
        debug_assert!(!data.is_empty());
        Ok(data)
      },
      Err(e) => match e {
        sqlx::Error::RowNotFound => Err(DatabaseError::RecordNotFound),
        _ => Err(e.into()),
      },
    }
  }

  async fn delete_collab(&self, object_id: &str) -> Result<()> {
    collaborate::delete_collab_with_cached_eviction(
      &self.pg_pool,
      self.redis_client.clone(),
      object_id,
    )
    .await?;
    Ok(())
  }

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> Result<()> {
    params.validate()?;

    collaborate::create_snapshot(
      &self.pg_pool,
      &params.object_id,
      &params.raw_data,
      &params.workspace_id.parse::<Uuid>()?,
    )
    .await?;
    Ok(())
  }

  async fn get_snapshot_data(&self, params: QuerySnapshotParams) -> Result<RawData> {
    let blob = collaborate::get_snapshot_blob(&self.pg_pool, params.snapshot_id).await?;
    Ok(blob)
  }

  async fn get_all_snapshots(
    &self,
    params: QueryObjectSnapshotParams,
  ) -> Result<AFCollabSnapshots> {
    let s = collaborate::get_all_snapshots(&self.pg_pool, &params.object_id).await?;
    Ok(s)
  }
}

const AF_COLLAB_TABLE: &str = "af_collab";

#[allow(dead_code)]
fn table_name(ty: &CollabType) -> String {
  match ty {
    CollabType::DatabaseRow => format!("{}_database_row", AF_COLLAB_TABLE),
    CollabType::Document => format!("{}_document", AF_COLLAB_TABLE),
    CollabType::Database => format!("{}_database", AF_COLLAB_TABLE),
    CollabType::WorkspaceDatabase => format!("{}_w_database", AF_COLLAB_TABLE),
    CollabType::Folder => format!("{}_folder", AF_COLLAB_TABLE),
    CollabType::UserAwareness => format!("{}_user_awareness", AF_COLLAB_TABLE),
  }
}
