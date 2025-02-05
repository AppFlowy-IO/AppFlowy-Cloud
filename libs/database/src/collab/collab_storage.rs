use app_error::AppError;
use async_trait::async_trait;

use database_entity::dto::{
  AFAccessLevel, AFSnapshotMeta, AFSnapshotMetas, CollabParams, InsertSnapshotParams, QueryCollab,
  QueryCollabParams, QueryCollabResult, SnapshotData,
};

use crate::collab::CollabType;
use collab::entity::EncodedCollab;
use serde::{Deserialize, Serialize};
use sqlx::Transaction;
use std::collections::HashMap;

pub const COLLAB_SNAPSHOT_LIMIT: i64 = 30;
pub const SNAPSHOT_PER_HOUR: i64 = 6;
pub type AppResult<T, E = AppError> = core::result::Result<T, E>;

/// [CollabStorageAccessControl] is a trait that provides access control when accessing the storage
/// of the Collab object.
#[async_trait]
pub trait CollabStorageAccessControl: Send + Sync + 'static {
  /// Updates the cache of the access level of the user for given collab object.
  async fn update_policy(&self, uid: &i64, oid: &str, level: AFAccessLevel)
    -> Result<(), AppError>;

  /// Removes the access level of the user for given collab object.
  async fn enforce_read_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
  ) -> Result<(), AppError>;

  /// Enforce the user's permission to write to the collab object.
  async fn enforce_write_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
  ) -> Result<(), AppError>;

  /// Enforce the user's permission to write to the workspace.
  async fn enforce_write_workspace(&self, uid: &i64, workspace_id: &str) -> Result<(), AppError>;

  /// Enforce the user's permission to delete the collab object.
  async fn enforce_delete(&self, workspace_id: &str, uid: &i64, oid: &str) -> Result<(), AppError>;
}

#[derive(Clone)]
pub enum GetCollabOrigin {
  User { uid: i64 },
  Server,
}

/// Represents a storage mechanism for collaborations.
///
/// This trait provides asynchronous methods for CRUD operations related to collaborations.
/// Implementors of this trait should provide the actual storage logic, be it in-memory, file-based, database-backed, etc.
#[async_trait]
pub trait CollabStorage: Send + Sync + 'static {
  /// Insert/update the collaboration object in the storage.
  /// # Arguments
  /// * `workspace_id` - The ID of the workspace.
  /// * `uid` - The ID of the user.
  /// * `params` - The parameters containing the data of the collaboration.
  /// * `flush_to_disk` - A boolean value that indicates whether the data should be written immediately.
  /// if write_immediately is true, the data will be written to disk immediately. Otherwise, the data will
  /// be scheduled to be written to disk later.
  ///
  async fn queue_insert_or_update_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    flush_to_disk: bool,
  ) -> AppResult<()>;

  async fn batch_insert_new_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: Vec<CollabParams>,
  ) -> AppResult<()>;

  /// Insert a new collaboration in the storage.
  ///
  /// # Arguments
  ///
  /// * `params` - The parameters required to create a new collaboration.
  ///
  /// # Returns
  ///
  /// * `Result<()>` - Returns `Ok(())` if the collaboration was created successfully, `Err` otherwise.
  async fn upsert_new_collab_with_transaction(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    action_description: &str,
  ) -> AppResult<()>;

  /// Retrieves a collaboration from the storage.
  ///
  /// # Arguments
  ///
  /// * `params` - The parameters required to query a collab object.
  ///
  /// # Returns
  ///
  /// * `Result<RawData>` - Returns the data of the collaboration if found, `Err` otherwise.
  async fn get_encode_collab(
    &self,
    origin: GetCollabOrigin,
    params: QueryCollabParams,
    from_editing_collab: bool,
  ) -> AppResult<EncodedCollab>;

  async fn batch_get_collab(
    &self,
    uid: &i64,
    workspace_id: &str,
    queries: Vec<QueryCollab>,
    from_editing_collab: bool,
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
  async fn delete_collab(&self, workspace_id: &str, uid: &i64, object_id: &str) -> AppResult<()>;

  async fn should_create_snapshot(&self, workspace_id: &str, oid: &str) -> Result<bool, AppError>;

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> AppResult<AFSnapshotMeta>;
  async fn queue_snapshot(&self, params: InsertSnapshotParams) -> AppResult<()>;

  async fn get_collab_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    snapshot_id: &i64,
  ) -> AppResult<SnapshotData>;

  async fn get_latest_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> AppResult<Option<SnapshotData>>;

  /// Returns list of snapshots for given object_id in descending order of creation time.
  async fn get_collab_snapshot_list(
    &self,
    workspace_id: &str,
    oid: &str,
  ) -> AppResult<AFSnapshotMetas>;
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CollabMetadata {
  pub object_id: String,
  pub workspace_id: String,
}
