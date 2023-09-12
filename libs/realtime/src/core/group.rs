use crate::collab::{CollabBroadcast, CollabGroup, CollabStoragePlugin};

use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab_define::CollabType;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use storage::collab::CollabStorage;

pub struct CollabGroupCache<S> {
  group_by_object_id: RwLock<HashMap<String, Arc<CollabGroup>>>,
  storage: S,
}

impl<S> CollabGroupCache<S>
where
  S: CollabStorage + Clone,
{
  pub fn new(storage: S) -> Self {
    Self {
      group_by_object_id: RwLock::new(HashMap::new()),
      storage,
    }
  }

  pub async fn create_group(&self, uid: i64, workspace_id: &str, object_id: &str) {
    if self.group_by_object_id.read().contains_key(object_id) {
      return;
    }

    let group = self.init_group(uid, workspace_id, object_id).await;
    self
      .group_by_object_id
      .write()
      .insert(object_id.to_string(), group);
  }

  async fn init_group(&self, uid: i64, workspace_id: &str, object_id: &str) -> Arc<CollabGroup> {
    tracing::trace!("Create new group for object_id:{}", object_id);

    let collab = MutexCollab::new(CollabOrigin::Server, object_id, vec![]);
    let broadcast = CollabBroadcast::new(object_id, collab.clone(), 10);
    let group = Arc::new(CollabGroup {
      collab: collab.clone(),
      broadcast,
      subscribers: Default::default(),
    });

    let plugin = CollabStoragePlugin::new(
      uid,
      workspace_id,
      CollabType::Document,
      self.storage.clone(),
      Arc::downgrade(&group),
    );
    collab.lock().add_plugin(Arc::new(plugin));
    collab.async_initialize().await;

    group
  }
}

impl<S> Deref for CollabGroupCache<S>
where
  S: CollabStorage,
{
  type Target = RwLock<HashMap<String, Arc<CollabGroup>>>;

  fn deref(&self) -> &Self::Target {
    &self.group_by_object_id
  }
}

impl<S> DerefMut for CollabGroupCache<S>
where
  S: CollabStorage,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.group_by_object_id
  }
}
