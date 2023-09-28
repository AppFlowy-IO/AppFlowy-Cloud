use crate::error::StorageError;
use anyhow::Context;
use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use collab_define::CollabType;
use sqlx::types::{chrono, Uuid};
use sqlx::{PgPool, Transaction};
use std::ops::DerefMut;
use std::str::FromStr;
use std::sync::Weak;
use storage_entity::{
  AFCollabSnapshot, AFCollabSnapshots, InsertCollabParams, InsertSnapshotParams, QueryCollabParams,
  QueryObjectSnapshotParams, QuerySnapshotParams, RawData,
};
use tracing::trace;

use validator::Validate;

pub type Result<T, E = StorageError> = core::result::Result<T, E>;

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

  async fn insert_collab(&self, params: InsertCollabParams) -> Result<()> {
    params.validate()?;

    let mut transaction = self
      .pg_pool
      .begin()
      .await
      .context("Failed to acquire a Postgres transaction to insert collab")?;
    insert_af_collab(&mut transaction, params).await?;
    transaction
      .commit()
      .await
      .context("Failed to commit transaction to insert collab")?;
    Ok(())
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
      None => Err(StorageError::RecordNotFound),
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
      None => Err(StorageError::RecordNotFound),
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

/// Inserts a new row into the `af_collab` table or updates an existing row if it matches the
/// provided `object_id`.Additionally, if the row is being inserted for the first time, a corresponding
/// entry will be added to the `af_collab_member` table.
///
/// # Arguments
///
/// * `tx` - A mutable reference to a PostgreSQL transaction.
/// * `params` - Parameters required for the insertion or update operation, encapsulated in
/// the `InsertCollabParams` struct.
///
/// # Returns
///
/// * `Ok(())` if the operation is successful.
/// * `Err(StorageError::Internal)` if there's an attempt to insert a row with an existing `object_id` but a different `workspace_id`.
/// * `Err(sqlx::Error)` for other database-related errors.
///
/// # Errors
///
/// This function will return an error if:
/// * There's a database operation failure.
/// * There's an attempt to insert a row with an existing `object_id` but a different `workspace_id`.
///
async fn insert_af_collab(
  tx: &mut Transaction<'_, sqlx::Postgres>,
  params: InsertCollabParams,
) -> Result<()> {
  let encrypt = 0;
  let partition_key = params.collab_type.value();
  let workspace_id = Uuid::from_str(&params.workspace_id)?;
  // Check if the row exists and get its workspace_id
  let existing_workspace_id: Option<Uuid> = sqlx::query_scalar!(
    "SELECT workspace_id FROM af_collab WHERE oid = $1",
    &params.object_id
  )
  .fetch_optional(tx.deref_mut())
  .await?;

  match existing_workspace_id {
    Some(existing_id) => {
      if existing_id == workspace_id {
        trace!("Update existing af_collab row");
        sqlx::query!(
          "UPDATE af_collab \
        SET blob = $2, len = $3, partition_key = $4, encrypt = $5, owner_uid = $6 WHERE oid = $1",
          params.object_id,
          params.raw_data,
          params.len,
          partition_key,
          encrypt,
          params.uid,
        )
        .execute(tx.deref_mut())
        .await
        .context(format!(
          "Update af_collab failed: {}:{}",
          params.uid, params.object_id
        ))?;
      } else {
        return Err(StorageError::Internal(anyhow::anyhow!(
          "Inserting a row with an existing object_id but different workspace_id"
        )));
      }
    },
    None => {
      // Get the 'Owner' role_id from af_roles
      let role_id: i32 = sqlx::query_scalar!("SELECT id FROM af_roles WHERE name = 'Owner'")
        .fetch_one(tx.deref_mut())
        .await?;

      trace!(
        "Insert new af_collab row: {}:{}:{}",
        params.uid,
        params.object_id,
        params.workspace_id
      );

      // Insert into af_collab_member
      sqlx::query!(
        "INSERT INTO af_collab_member (oid, uid, role_id) VALUES ($1, $2, $3)",
        params.object_id,
        params.uid,
        role_id
      )
      .execute(tx.deref_mut())
      .await
      .context(format!(
        "Insert af_collab_member failed: {}:{}:{}",
        params.uid, params.object_id, role_id
      ))?;

      sqlx::query!(
        "INSERT INTO af_collab (oid, blob, len, partition_key, encrypt, owner_uid, workspace_id)\
          VALUES ($1, $2, $3, $4, $5, $6, $7)",
        params.object_id,
        params.raw_data,
        params.len,
        partition_key,
        encrypt,
        params.uid,
        workspace_id,
      )
      .execute(tx.deref_mut())
      .await
      .context(format!(
        "Insert new af_collab failed: {}:{}:{}",
        params.uid, params.object_id, params.collab_type
      ))?;
    },
  }

  Ok(())
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
