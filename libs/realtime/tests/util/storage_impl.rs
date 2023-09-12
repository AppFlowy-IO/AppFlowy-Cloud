use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use storage::collab::{CollabStorage, RawData};
use storage::entities::{InsertCollabParams, QueryCollabParams};
use storage::error::StorageError;
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct CollabMemoryStorageImpl {
  config: storage::collab::Config,
  collab_data_by_object_id: Arc<RwLock<HashMap<String, RawData>>>,
}

impl CollabMemoryStorageImpl {
  pub fn new(config: storage::collab::Config) -> Self {
    Self {
      config,
      collab_data_by_object_id: Arc::new(RwLock::new(HashMap::new())),
    }
  }
}

#[async_trait]
impl CollabStorage for CollabMemoryStorageImpl {
  fn config(&self) -> &storage::collab::Config {
    &self.config
  }

  async fn is_exist(&self, object_id: &str) -> bool {
    self
      .collab_data_by_object_id
      .read()
      .await
      .contains_key(object_id)
  }

  async fn insert_collab(&self, params: InsertCollabParams) -> storage::collab::Result<()> {
    self
      .collab_data_by_object_id
      .write()
      .await
      .insert(params.object_id.to_string(), params.raw_data);
    Ok(())
  }

  async fn get_collab(&self, params: QueryCollabParams) -> storage::collab::Result<RawData> {
    self
      .collab_data_by_object_id
      .read()
      .await
      .get(&params.object_id)
      .cloned()
      .ok_or(StorageError::RecordNotFound)
  }

  async fn delete_collab(&self, object_id: &str) -> storage::collab::Result<()> {
    self
      .collab_data_by_object_id
      .write()
      .await
      .remove(object_id);
    Ok(())
  }
}
