use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use collab_define::CollabType;
use database_entity::error::DatabaseError;
use database_entity::{
  AFCollabSnapshot, AFCollabSnapshots, QueryObjectSnapshotParams, QuerySnapshotParams,
};
use shared_entity::dto::{InsertCollabParams, InsertSnapshotParams, QueryCollabParams};
use sqlx::types::{chrono, Uuid};
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Weak;
use validator::Validate;
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
  async fn insert_collab(&self, params: InsertCollabParams) -> Result<()>;

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
pub struct CollabPostgresDBStorageImpl {
  #[allow(dead_code)]
  pg_pool: PgPool,
  config: StorageConfig,
}

pub const FLUSH_PER_UPDATE: u32 = 100;
impl CollabPostgresDBStorageImpl {
  pub fn new(pg_pool: PgPool) -> Self {
    let config = StorageConfig {
      flush_per_update: FLUSH_PER_UPDATE,
    };
    Self { pg_pool, config }
  }
}

#[async_trait]
impl CollabStorage for CollabPostgresDBStorageImpl {
  fn config(&self) -> &StorageConfig {
    &self.config
  }

  async fn is_exist(&self, object_id: &str) -> bool {
    let result = sqlx::query!(
      "SELECT oid FROM af_collab WHERE oid = $1 LIMIT 1",
      object_id
    )
    .fetch_optional(&self.pg_pool)
    .await
    .unwrap_or(None);
    result.is_some()
  }

  async fn insert_collab(&self, _params: InsertCollabParams) -> Result<()> {
    todo!()
    // params.validate()?;

    // let mut transaction = self
    //   .pg_pool
    //   .begin()
    //   .await
    //   .context("Failed to acquire a Postgres transaction to insert collab")?;
    // insert_af_collab(&mut transaction, params).await?;
    // transaction
    //   .commit()
    //   .await
    //   .context("Failed to commit transaction to insert collab")?;
    // Ok(())
  }

  async fn get_collab(&self, params: QueryCollabParams) -> Result<RawData> {
    params.validate()?;

    let partition_key = params.collab_type.value();
    let record = sqlx::query!(
      r#"
        SELECT blob
        FROM af_collab
        WHERE oid = $1 AND partition_key = $2 AND deleted_at IS NULL;
        "#,
      params.object_id,
      partition_key,
    )
    .fetch_optional(&self.pg_pool)
    .await?;

    match record {
      Some(record) => {
        debug_assert!(!record.blob.is_empty());
        Ok(record.blob)
      },
      None => Err(DatabaseError::RecordNotFound),
    }
  }

  async fn delete_collab(&self, object_id: &str) -> Result<()> {
    let delete_time = chrono::Utc::now();
    sqlx::query!(
      r#"
        UPDATE af_collab
        SET deleted_at = $2
        WHERE oid = $1;
        "#,
      object_id,
      delete_time
    )
    .execute(&self.pg_pool)
    .await?;
    Ok(())
  }

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> Result<()> {
    params.validate()?;

    let encrypt = 0;
    let workspace_id = Uuid::from_str(&params.workspace_id)?;
    sqlx::query!(
      r#"
        INSERT INTO af_collab_snapshot (oid, blob, len, encrypt, workspace_id)
        VALUES ($1, $2, $3, $4, $5)
        "#,
      params.object_id,
      params.raw_data,
      params.len,
      encrypt,
      workspace_id,
    )
    .execute(&self.pg_pool)
    .await?;
    Ok(())
  }

  async fn get_snapshot_data(&self, params: QuerySnapshotParams) -> Result<RawData> {
    let record = sqlx::query!(
      r#"
        SELECT blob
        FROM af_collab_snapshot
        WHERE sid = $1 AND deleted_at IS NULL;
        "#,
      params.snapshot_id,
    )
    .fetch_optional(&self.pg_pool)
    .await?;

    match record {
      Some(record) => Ok(record.blob),
      None => Err(DatabaseError::RecordNotFound),
    }
  }

  async fn get_all_snapshots(
    &self,
    params: QueryObjectSnapshotParams,
  ) -> Result<AFCollabSnapshots> {
    let snapshots: Vec<AFCollabSnapshot> = sqlx::query_as!(
      AFCollabSnapshot,
      r#"
        SELECT sid as "snapshot_id", oid as "object_id", created_at
        FROM af_collab_snapshot where oid = $1 AND deleted_at IS NULL;
        "#,
      params.object_id
    )
    .fetch_all(&self.pg_pool)
    .await?;
    Ok(AFCollabSnapshots(snapshots))
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
