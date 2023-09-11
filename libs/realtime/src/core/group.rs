use crate::collab::{CollabBroadcast, CollabGroup, CollabStoragePlugin};

use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use storage::collab::CollabStorage;

pub struct CollabGroupCache<S> {
  collab_group_by_object_id: RwLock<HashMap<String, CollabGroup>>,
  storage: S,
}

impl<S> CollabGroupCache<S>
where
  S: CollabStorage + Clone,
{
  pub fn new(storage: S) -> Self {
    Self {
      collab_group_by_object_id: RwLock::new(HashMap::new()),
      storage,
    }
  }

  pub async fn create_group(&self, workspace_id: &str, object_id: &str) {
    if self
      .collab_group_by_object_id
      .read()
      .contains_key(object_id)
    {
      return;
    }

    let group = self.init_group(workspace_id, object_id).await;
    self
      .collab_group_by_object_id
      .write()
      .insert(object_id.to_string(), group);
  }

  async fn init_group(&self, workspace_id: &str, object_id: &str) -> CollabGroup {
    tracing::trace!("Create new group for object_id:{}", object_id);

    let collab = MutexCollab::new(CollabOrigin::Server, object_id, vec![]);
    let plugin = CollabStoragePlugin::new(workspace_id, self.storage.clone()).unwrap();
    collab.lock().add_plugin(Arc::new(plugin));
    collab.async_initialize().await;

    let broadcast = CollabBroadcast::new(object_id, collab.clone(), 10);
    CollabGroup {
      collab,
      broadcast,
      subscribers: Default::default(),
    }
  }
}

impl<S> Deref for CollabGroupCache<S>
where
  S: CollabStorage,
{
  type Target = RwLock<HashMap<String, CollabGroup>>;

  fn deref(&self) -> &Self::Target {
    &self.collab_group_by_object_id
  }
}

impl<S> DerefMut for CollabGroupCache<S>
where
  S: CollabStorage,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.collab_group_by_object_id
  }
}
