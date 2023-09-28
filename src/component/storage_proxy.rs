use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use database::collab::{CollabPostgresDBStorageImpl, CollabStorage, StorageConfig};
use database_entity::{
  AFCollabSnapshots, InsertCollabParams, InsertSnapshotParams, QueryCollabParams,
  QueryObjectSnapshotParams, QuerySnapshotParams, RawData,
};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tokio::sync::RwLock;
use tracing::info;

#[derive(Clone)]
pub struct CollabStorageProxy {
  inner: CollabPostgresDBStorageImpl,
  collab_by_object_id: Arc<RwLock<HashMap<String, Weak<MutexCollab>>>>,
}

impl CollabStorageProxy {
  pub fn new(inner: CollabPostgresDBStorageImpl) -> Self {
    Self {
      inner,
      collab_by_object_id: Arc::new(RwLock::new(HashMap::new())),
    }
  }
}

#[async_trait]
impl CollabStorage for CollabStorageProxy {
  fn config(&self) -> &StorageConfig {
    self.inner.config()
  }

  async fn is_exist(&self, object_id: &str) -> bool {
    self.inner.is_exist(object_id).await
  }

  async fn cache_collab(&self, object_id: &str, collab: Weak<MutexCollab>) {
    tracing::trace!("Cache collab:{} in memory", object_id);
    self
      .collab_by_object_id
      .write()
      .await
      .insert(object_id.to_string(), collab);
  }

  async fn insert_collab(&self, params: InsertCollabParams) -> database::collab::Result<()> {
    self.inner.insert_collab(params).await
  }

  async fn get_collab(&self, params: QueryCollabParams) -> database::collab::Result<RawData> {
    let collab = self
      .collab_by_object_id
      .read()
      .await
      .get(&params.object_id)
      .and_then(|collab| collab.upgrade());

    match collab {
      None => self.inner.get_collab(params).await,
      Some(collab) => {
        info!("Get collab data:{} from memory", params.object_id);
        Ok(collab.encode_as_update_v1().0)
      },
    }
  }

  async fn delete_collab(&self, object_id: &str) -> database::collab::Result<()> {
    self.inner.delete_collab(object_id).await
  }

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> database::collab::Result<()> {
    self.inner.create_snapshot(params).await
  }

  async fn get_snapshot_data(
    &self,
    params: QuerySnapshotParams,
  ) -> database::collab::Result<RawData> {
    self.inner.get_snapshot_data(params).await
  }

  async fn get_all_snapshots(
    &self,
    params: QueryObjectSnapshotParams,
  ) -> database::collab::Result<AFCollabSnapshots> {
    self.inner.get_all_snapshots(params).await
  }
}
