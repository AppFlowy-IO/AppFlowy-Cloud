use app_error::AppError;
use async_trait::async_trait;

use database_entity::dto::{
  AFAccessLevel, AFSnapshotMeta, AFSnapshotMetas, CollabParams, InsertSnapshotParams, QueryCollab,
  QueryCollabParams, QueryCollabResult, SnapshotData,
};

use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use serde::{Deserialize, Serialize};
use sqlx::Transaction;
use std::collections::HashMap;
use std::sync::Arc;

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
  ) -> Result<bool, AppError>;

  /// Enforce the user's permission to write to the collab object.
  async fn enforce_write_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
  ) -> Result<bool, AppError>;

  /// Enforce the user's permission to write to the workspace.
  async fn enforce_write_workspace(&self, uid: &i64, workspace_id: &str) -> Result<bool, AppError>;

  /// Enforce the user's permission to delete the collab object.
  async fn enforce_delete(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
  ) -> Result<bool, AppError>;
}

/// Represents a storage mechanism for collaborations.
///
/// This trait provides asynchronous methods for CRUD operations related to collaborations.
/// Implementors of this trait should provide the actual storage logic, be it in-memory, file-based, database-backed, etc.
#[async_trait]
pub trait CollabStorage: Send + Sync + 'static {
  fn encode_collab_redis_query_state(&self) -> (u64, u64);

  /// Insert/update the collaboration object in the storage.
  /// # Arguments
  /// * `workspace_id` - The ID of the workspace.
  /// * `uid` - The ID of the user.
  /// * `params` - The parameters containing the data of the collaboration.
  /// * `write_immediately` - A boolean value that indicates whether the data should be written immediately.
  /// if write_immediately is true, the data will be written to disk immediately. Otherwise, the data will
  /// be scheduled to be written to disk later.
  ///
  async fn insert_or_update_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    write_immediately: bool,
  ) -> AppResult<()>;

  async fn insert_new_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
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
  async fn insert_new_collab_with_transaction(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
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
    uid: &i64,
    params: QueryCollabParams,
    is_collab_init: bool,
  ) -> AppResult<EncodedCollab>;

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
  async fn delete_collab(&self, workspace_id: &str, uid: &i64, object_id: &str) -> AppResult<()>;

  async fn query_collab_meta(
    &self,
    object_id: &str,
    collab_type: &CollabType,
  ) -> AppResult<CollabMetadata>;
  async fn should_create_snapshot(&self, oid: &str) -> Result<bool, AppError>;

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> AppResult<AFSnapshotMeta>;
  async fn queue_snapshot(&self, params: InsertSnapshotParams) -> AppResult<()>;

  async fn get_collab_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    snapshot_id: &i64,
  ) -> AppResult<SnapshotData>;

  /// Returns list of snapshots for given object_id in descending order of creation time.
  async fn get_collab_snapshot_list(&self, oid: &str) -> AppResult<AFSnapshotMetas>;

  async fn add_connected_user(&self, uid: i64, device_id: &str);
  async fn remove_connected_user(&self, uid: i64, device_id: &str);
}

#[async_trait]
impl<T> CollabStorage for Arc<T>
where
  T: CollabStorage,
{
  fn encode_collab_redis_query_state(&self) -> (u64, u64) {
    self.as_ref().encode_collab_redis_query_state()
  }

  async fn insert_or_update_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    write_immediately: bool,
  ) -> AppResult<()> {
    self
      .as_ref()
      .insert_or_update_collab(workspace_id, uid, params, write_immediately)
      .await
  }

  async fn insert_new_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
  ) -> AppResult<()> {
    self
      .as_ref()
      .insert_new_collab(workspace_id, uid, params)
      .await
  }

  async fn insert_new_collab_with_transaction(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> AppResult<()> {
    self
      .as_ref()
      .insert_new_collab_with_transaction(workspace_id, uid, params, transaction)
      .await
  }

  async fn get_encode_collab(
    &self,
    uid: &i64,
    params: QueryCollabParams,
    is_collab_init: bool,
  ) -> AppResult<EncodedCollab> {
    self
      .as_ref()
      .get_encode_collab(uid, params, is_collab_init)
      .await
  }

  async fn batch_get_collab(
    &self,
    uid: &i64,
    queries: Vec<QueryCollab>,
  ) -> HashMap<String, QueryCollabResult> {
    self.as_ref().batch_get_collab(uid, queries).await
  }

  async fn delete_collab(&self, workspace_id: &str, uid: &i64, object_id: &str) -> AppResult<()> {
    self
      .as_ref()
      .delete_collab(workspace_id, uid, object_id)
      .await
  }

  async fn query_collab_meta(
    &self,
    object_id: &str,
    collab_type: &CollabType,
  ) -> AppResult<CollabMetadata> {
    self
      .as_ref()
      .query_collab_meta(object_id, collab_type)
      .await
  }

  async fn should_create_snapshot(&self, oid: &str) -> Result<bool, AppError> {
    self.as_ref().should_create_snapshot(oid).await
  }

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> AppResult<AFSnapshotMeta> {
    self.as_ref().create_snapshot(params).await
  }

  async fn queue_snapshot(&self, params: InsertSnapshotParams) -> AppResult<()> {
    self.as_ref().queue_snapshot(params).await
  }

  async fn get_collab_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    snapshot_id: &i64,
  ) -> AppResult<SnapshotData> {
    self
      .as_ref()
      .get_collab_snapshot(workspace_id, object_id, snapshot_id)
      .await
  }

  async fn get_collab_snapshot_list(&self, oid: &str) -> AppResult<AFSnapshotMetas> {
    self.as_ref().get_collab_snapshot_list(oid).await
  }

  async fn add_connected_user(&self, uid: i64, device_id: &str) {
    self.as_ref().add_connected_user(uid, device_id).await
  }

  async fn remove_connected_user(&self, uid: i64, device_id: &str) {
    self.as_ref().remove_connected_user(uid, device_id).await
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CollabMetadata {
  pub object_id: String,
  pub workspace_id: String,
}
