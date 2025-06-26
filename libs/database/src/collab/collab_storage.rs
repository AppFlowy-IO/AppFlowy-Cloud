use app_error::AppError;
use async_trait::async_trait;

use database_entity::dto::{AFAccessLevel, CollabParams, QueryCollab, QueryCollabResult};

use appflowy_proto::TimestampedEncodedCollab;
use collab_entity::CollabType;
use serde::{Deserialize, Serialize};
use sqlx::Transaction;
use std::collections::HashMap;
use uuid::Uuid;

pub const COLLAB_SNAPSHOT_LIMIT: i64 = 30;
pub const SNAPSHOT_PER_HOUR: i64 = 6;
pub type AppResult<T, E = AppError> = core::result::Result<T, E>;

/// [CollabStorageAccessControl] is a trait that provides access control when accessing the storage
/// of the Collab object.
#[async_trait]
pub trait CollabStorageAccessControl: Send + Sync + 'static {
  /// Updates the cache of the access level of the user for given collab object.
  async fn update_policy(
    &self,
    uid: &i64,
    oid: &Uuid,
    level: AFAccessLevel,
  ) -> Result<(), AppError>;

  /// Removes the access level of the user for given collab object.
  async fn enforce_read_collab(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    oid: &Uuid,
  ) -> Result<(), AppError>;

  /// Enforce the user's permission to write to the collab object.
  async fn enforce_write_collab(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    oid: &Uuid,
  ) -> Result<(), AppError>;

  /// Enforce the user's permission to write to the workspace.
  async fn enforce_write_workspace(&self, uid: &i64, workspace_id: &Uuid) -> Result<(), AppError>;

  /// Enforce the user's permission to delete the collab object.
  async fn enforce_delete(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    oid: &Uuid,
  ) -> Result<(), AppError>;
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
  async fn upsert_collab(
    &self,
    workspace_id: Uuid,
    uid: &i64,
    params: CollabParams,
  ) -> AppResult<()>;

  async fn upsert_collab_background(
    &self,
    workspace_id: Uuid,
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
  async fn upsert_new_collab_with_transaction(
    &self,
    workspace_id: Uuid,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    action_description: &str,
  ) -> AppResult<()>;

  async fn batch_insert_new_collab(
    &self,
    workspace_id: Uuid,
    uid: &i64,
    params: Vec<CollabParams>,
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
  async fn get_full_encode_collab(
    &self,
    origin: GetCollabOrigin,
    workspace_id: &Uuid,
    object_id: &Uuid,
    collab_type: CollabType,
  ) -> AppResult<TimestampedEncodedCollab>;

  async fn batch_get_collab(
    &self,
    uid: &i64,
    workspace_id: Uuid,
    queries: Vec<QueryCollab>,
  ) -> HashMap<Uuid, QueryCollabResult>;

  /// Deletes a collaboration from the storage.
  ///
  /// # Arguments
  ///
  /// * `object_id` - A string slice that holds the ID of the collaboration to delete.
  ///
  /// # Returns
  ///
  /// * `Result<()>` - Returns `Ok(())` if the collaboration was deleted successfully, `Err` otherwise.
  async fn delete_collab(&self, workspace_id: &Uuid, uid: &i64, object_id: &Uuid) -> AppResult<()>;

  fn mark_as_editing(&self, oid: Uuid);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollabMetadata {
  #[serde(with = "uuid_str")]
  pub object_id: Uuid,
  #[serde(with = "uuid_str")]
  pub workspace_id: Uuid,
}

mod uuid_str {
  use serde::Deserialize;
  use uuid::Uuid;

  pub fn serialize<S>(uuid: &Uuid, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    serializer.serialize_str(&uuid.to_string())
  }

  pub fn deserialize<'de, D>(deserializer: D) -> Result<Uuid, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let s = String::deserialize(deserializer)?;
    Uuid::parse_str(&s).map_err(serde::de::Error::custom)
  }
}
