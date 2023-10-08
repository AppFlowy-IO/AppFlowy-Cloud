use crate::collaborate::{CollabBroadcast, CollabStoragePlugin, Subscription};
use crate::entities::RealtimeUser;
use anyhow::Error;
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_define::CollabType;
use database::collab::CollabStorage;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::spawn_blocking;

use tracing::{error, warn};

pub struct CollabGroupCache<S, U> {
  group_by_object_id: Arc<RwLock<HashMap<String, Arc<CollabGroup<U>>>>>,
  storage: S,
}

impl<S, U> CollabGroupCache<S, U>
where
  S: CollabStorage + Clone,
  U: RealtimeUser,
{
  pub fn new(storage: S) -> Self {
    Self {
      group_by_object_id: Arc::new(RwLock::new(HashMap::new())),
      storage,
    }
  }

  pub async fn contains_user(&self, object_id: &str, user: &U) -> Result<bool, Error> {
    let group_by_object_id = self.group_by_object_id.read().await;
    if let Some(group) = group_by_object_id.get(object_id) {
      Ok(group.subscribers.read().await.get(user).is_some())
    } else {
      Ok(false)
    }
  }

  pub async fn contains_group(&self, object_id: &str) -> Result<bool, Error> {
    let group_by_object_id = self.group_by_object_id.try_read()?;
    Ok(group_by_object_id.get(object_id).is_some())
  }

  pub async fn get_group(&self, object_id: &str) -> Option<Arc<CollabGroup<U>>> {
    self.group_by_object_id.read().await.get(object_id).cloned()
  }

  pub async fn remove_group(&self, object_id: &str) {
    match self.group_by_object_id.try_write() {
      Ok(mut group_by_object_id) => {
        group_by_object_id.remove(object_id);
      },
      Err(err) => error!("Failed to acquire write lock to remove group: {:?}", err),
    }
  }

  pub async fn create_group(
    &self,
    uid: i64,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) {
    match self.group_by_object_id.try_write() {
      Ok(mut group_by_object_id) => {
        if group_by_object_id.contains_key(object_id) {
          warn!("Group for object_id:{} already exists", object_id);
          return;
        }

        let group = self
          .init_group(uid, workspace_id, object_id, collab_type)
          .await;
        group_by_object_id.insert(object_id.to_string(), group);
      },
      Err(err) => error!("Failed to acquire write lock to create group: {:?}", err),
    }
  }

  #[tracing::instrument(skip(self))]
  async fn init_group(
    &self,
    uid: i64,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Arc<CollabGroup<U>> {
    tracing::trace!("Create new group for object_id:{}", object_id);
    let collab = MutexCollab::new(CollabOrigin::Server, object_id, vec![]);
    let broadcast = CollabBroadcast::new(object_id, collab.clone(), 10);
    let collab = Arc::new(collab.clone());

    // The lifecycle of the collab is managed by the group.
    let group = Arc::new(CollabGroup {
      collab: collab.clone(),
      broadcast,
      subscribers: Default::default(),
    });

    let plugin = CollabStoragePlugin::new(
      uid,
      workspace_id,
      collab_type,
      self.storage.clone(),
      Arc::downgrade(&group),
    );
    collab.lock().add_plugin(Arc::new(plugin));
    collab.lock_arc().initialize().await;

    self
      .storage
      .cache_collab(object_id, Arc::downgrade(&collab))
      .await;
    group
  }
}

/// A group used to manage a single [Collab] object
pub struct CollabGroup<U> {
  pub collab: Arc<MutexCollab>,

  /// A broadcast used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
  /// to subscribes.
  pub broadcast: CollabBroadcast,

  /// A list of subscribers to this group. Each subscriber will receive updates from the
  /// broadcast.
  pub subscribers: RwLock<HashMap<U, Subscription>>,
}

impl<U> CollabGroup<U>
where
  U: RealtimeUser,
{
  /// Mutate the [Collab] by the given closure
  pub fn get_mut_collab<F>(&self, f: F)
  where
    F: FnOnce(&Collab),
  {
    let collab = self.collab.lock();
    f(&collab);
  }

  pub async fn is_empty(&self) -> bool {
    self.subscribers.read().await.is_empty()
  }

  /// Flush the [Collab] to the storage.
  /// When there is no subscriber, perform the flush in a blocking task.
  pub fn save_collab(&self) {
    let collab = self.collab.clone();
    spawn_blocking(move || {
      collab.lock().flush();
    });
  }
}
