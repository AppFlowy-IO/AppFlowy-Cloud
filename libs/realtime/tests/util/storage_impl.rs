use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use storage::collab::Result;
use storage::collab::{CollabStorage, StorageConfig};
use storage::error::StorageError;
use storage_entity::{
  AFCollabSnapshots, InsertCollabParams, InsertSnapshotParams, QueryCollabParams,
  QueryObjectSnapshotParams, QuerySnapshotParams, RawData,
};
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct CollabMemoryStorageImpl {
  config: StorageConfig,
  collab_data_by_object_id: Arc<RwLock<HashMap<String, RawData>>>,
}

impl CollabMemoryStorageImpl {
  pub fn new(config: StorageConfig) -> Self {
    Self {
      config,
      collab_data_by_object_id: Arc::new(RwLock::new(HashMap::new())),
    }
  }
}

#[async_trait]
impl CollabStorage for CollabMemoryStorageImpl {
  fn config(&self) -> &StorageConfig {
    &self.config
  }

  async fn is_exist(&self, object_id: &str) -> bool {
    self
      .collab_data_by_object_id
      .read()
      .await
      .contains_key(object_id)
  }

  async fn insert_collab(&self, params: InsertCollabParams) -> Result<()> {
    self
      .collab_data_by_object_id
      .write()
      .await
      .insert(params.object_id.to_string(), params.raw_data);
    Ok(())
  }

  async fn get_collab(&self, params: QueryCollabParams) -> Result<RawData> {
    self
      .collab_data_by_object_id
      .read()
      .await
      .get(&params.object_id)
      .cloned()
      .ok_or(StorageError::RecordNotFound)
  }

  async fn delete_collab(&self, object_id: &str) -> Result<()> {
    self
      .collab_data_by_object_id
      .write()
      .await
      .remove(object_id);
    Ok(())
  }

  async fn create_snapshot(&self, _params: InsertSnapshotParams) -> Result<()> {
    Ok(())
  }

  async fn get_snapshot_data(&self, _params: QuerySnapshotParams) -> Result<RawData> {
    Ok(vec![])
  }

  async fn get_all_snapshots(
    &self,
    _params: QueryObjectSnapshotParams,
  ) -> Result<AFCollabSnapshots> {
    Ok(AFCollabSnapshots(vec![]))
  }
}
