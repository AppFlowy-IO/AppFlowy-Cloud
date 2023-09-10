use crate::entities::CreateCollabParams;
use crate::error::StorageError;
use async_trait::async_trait;
use sqlx::PgPool;

pub type Result<T, E = StorageError> = core::result::Result<T, E>;

pub type RawData = Vec<u8>;

/// Represents a storage mechanism for collaborations.
///
/// This trait provides asynchronous methods for CRUD operations related to collaborations.
/// Implementors of this trait should provide the actual storage logic, be it in-memory, file-based, database-backed, etc.
#[async_trait]
pub trait CollabStorage: Clone + Send + Sync + 'static {
  /// Checks if a collaboration with the given object ID exists in the storage.
  ///
  /// # Arguments
  ///
  /// * `object_id` - A string slice that holds the ID of the collaboration.
  ///
  /// # Returns
  ///
  /// * `bool` - `true` if the collaboration exists, `false` otherwise.
  async fn is_exist(&self, _object_id: &str) -> bool;

  /// Creates a new collaboration in the storage.
  ///
  /// # Arguments
  ///
  /// * `params` - The parameters required to create a new collaboration.
  ///
  /// # Returns
  ///
  /// * `Result<()>` - Returns `Ok(())` if the collaboration was created successfully, `Err` otherwise.
  async fn create_collab(&self, _params: CreateCollabParams) -> Result<()>;

  /// Updates an existing collaboration in the storage.
  ///
  /// # Arguments
  ///
  /// * `_object_id` - A string slice that holds the ID of the collaboration to update.
  /// * `_data` - The new data for the collaboration.
  ///
  /// # Returns
  ///
  /// * `Result<()>` - Returns `Ok(())` if the collaboration was updated successfully, `Err` otherwise.
  async fn update_collab(&self, object_id: &str, _data: RawData) -> Result<()>;

  /// Retrieves a collaboration from the storage.
  ///
  /// # Arguments
  ///
  /// * `_object_id` - A string slice that holds the ID of the collaboration to retrieve.
  ///
  /// # Returns
  ///
  /// * `Result<RawData>` - Returns the data of the collaboration if found, `Err` otherwise.
  async fn get_collab(&self, object_id: &str) -> Result<RawData>;

  /// Deletes a collaboration from the storage.
  ///
  /// # Arguments
  ///
  /// * `_object_id` - A string slice that holds the ID of the collaboration to delete.
  ///
  /// # Returns
  ///
  /// * `Result<()>` - Returns `Ok(())` if the collaboration was deleted successfully, `Err` otherwise.
  async fn delete_collab(&self, object_id: &str) -> Result<()>;
}

#[derive(Clone)]
pub struct CollabStoragePGImpl {
  #[allow(dead_code)]
  pg_pool: PgPool,
}

impl CollabStoragePGImpl {
  pub fn new(pg_pool: PgPool) -> Self {
    Self { pg_pool }
  }
}

#[async_trait]
impl CollabStorage for CollabStoragePGImpl {
  async fn is_exist(&self, _object_id: &str) -> bool {
    todo!()
  }

  async fn create_collab(&self, _params: CreateCollabParams) -> Result<()> {
    todo!()
  }

  async fn update_collab(&self, _object_id: &str, _data: RawData) -> Result<()> {
    todo!()
  }

  async fn get_collab(&self, _object_id: &str) -> Result<RawData> {
    todo!()
  }

  async fn delete_collab(&self, _object_id: &str) -> Result<()> {
    todo!()
  }
}
