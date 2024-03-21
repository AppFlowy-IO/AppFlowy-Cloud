use crate::collaborate::group::CollabGroup;
use crate::collaborate::plugin::CollabStoragePlugin;
use crate::RealtimeAccessControl;
use anyhow::Error;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_rt_entity::user::RealtimeUser;
use dashmap::DashMap;
use database::collab::CollabStorage;
use std::sync::Arc;
use tracing::{debug, error, instrument};

pub struct AllGroup<S, U, AC> {
  group_by_object_id: Arc<DashMap<String, Arc<CollabGroup<U>>>>,
  storage: Arc<S>,
  access_control: Arc<AC>,
}

impl<S, U, AC> AllGroup<S, U, AC>
where
  S: CollabStorage,
  U: RealtimeUser,
  AC: RealtimeAccessControl,
{
  pub fn new(storage: Arc<S>, access_control: Arc<AC>) -> Self {
    Self {
      group_by_object_id: Arc::new(DashMap::new()),
      storage,
      access_control,
    }
  }

  /// Performs a periodic check to remove groups based on the following conditions:
  /// Groups that have been inactive for a specified period of time.
  pub async fn tick(&self) -> Vec<String> {
    let mut inactive_group_ids = vec![];
    for entry in self.group_by_object_id.iter() {
      let (object_id, group) = (entry.key(), entry.value());
      if group.is_inactive().await {
        inactive_group_ids.push(object_id.clone());
        if inactive_group_ids.len() > 5 {
          break;
        }
      }
    }

    if !inactive_group_ids.is_empty() {
      for object_id in &inactive_group_ids {
        self.remove_group(object_id).await;
      }
    }
    inactive_group_ids
  }

  pub async fn contains_user(&self, object_id: &str, user: &U) -> bool {
    if let Some(entry) = self.group_by_object_id.get(object_id) {
      entry.value().contains_user(user)
    } else {
      false
    }
  }

  pub async fn remove_user(&self, object_id: &str, user: &U) -> Result<(), Error> {
    if let Some(entry) = self.group_by_object_id.get(object_id) {
      let group = entry.value();
      group.remove_user(user).await;
    }
    Ok(())
  }

  pub async fn contains_group(&self, object_id: &str) -> bool {
    self.group_by_object_id.get(object_id).is_some()
  }

  pub async fn get_group(&self, object_id: &str) -> Option<Arc<CollabGroup<U>>> {
    self
      .group_by_object_id
      .get(object_id)
      .map(|v| v.value().clone())
  }

  #[instrument(skip(self))]
  async fn remove_group(&self, object_id: &str) {
    let entry = self.group_by_object_id.remove(object_id);
    if let Some(entry) = entry {
      let group = entry.1;
      group.stop().await;
      group.flush_collab().await;
    } else {
      // Log error if the group doesn't exist
      error!("Group for object_id:{} not found", object_id);
    }
  }

  pub async fn create_group(
    &self,
    uid: i64,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) {
    let mut collab = Collab::new_with_origin(CollabOrigin::Server, object_id, vec![], false);
    let plugin = CollabStoragePlugin::new(
      uid,
      workspace_id,
      collab_type.clone(),
      self.storage.clone(),
      self.access_control.clone(),
    );
    collab.add_plugin(Box::new(plugin));
    collab.initialize().await;

    // The lifecycle of the collab is managed by the group.
    let group = Arc::new(
      CollabGroup::new(
        workspace_id.to_string(),
        object_id.to_string(),
        collab_type,
        collab,
      )
      .await,
    );
    debug!("[realtime]: {} create group:{}", uid, object_id);
    self.group_by_object_id.insert(object_id.to_string(), group);
  }

  pub async fn number_of_groups(&self) -> usize {
    self.group_by_object_id.len()
  }
}
