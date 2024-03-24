use crate::collaborate::group::CollabGroup;
use crate::collaborate::plugin::CollabStoragePlugin;
use crate::RealtimeAccessControl;

use crate::client_msg_router::ClientMessageRouter;
use crate::error::RealtimeError;
use anyhow::anyhow;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_rt_entity::collab_msg::CollabMessage;
use collab_rt_entity::user::{Editing, RealtimeUser};
use dashmap::DashMap;
use database::collab::CollabStorage;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use tracing::{debug, error, event, instrument, trace};

pub struct AllGroup<S, AC> {
  group_by_object_id: Rc<DashMap<String, Rc<CollabGroup>>>,
  /// Keep track of all [Collab] objects that a user is subscribed to.
  editing_by_user: Arc<DashMap<RealtimeUser, HashSet<Editing>>>,
  storage: Arc<S>,
  access_control: Arc<AC>,
}

impl<S, AC> AllGroup<S, AC>
where
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  pub fn new(storage: Arc<S>, access_control: Arc<AC>) -> Self {
    Self {
      group_by_object_id: Rc::new(DashMap::new()),
      editing_by_user: Arc::new(Default::default()),
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

  pub async fn contains_user(&self, object_id: &str, user: &RealtimeUser) -> bool {
    if let Some(entry) = self.group_by_object_id.get(object_id) {
      entry.value().contains_user(user)
    } else {
      false
    }
  }

  pub async fn remove_user(&self, user: &RealtimeUser) {
    let entry = self.editing_by_user.remove(user);
    if let Some(entry) = entry {
      for editing in entry.1 {
        if let Some(entry) = self.group_by_object_id.get(&editing.object_id) {
          let group = entry.value();
          group.remove_user(user).await;
        }

        if cfg!(debug_assertions) {
          // Remove the user from the group and remove the group from the cache if the group is empty.
          if let Some(group) = self.get_group(&editing.object_id).await {
            event!(
              tracing::Level::TRACE,
              "{}: Remove group subscriber:{}, Current group member: {}",
              &editing.object_id,
              editing.origin,
              group.user_count(),
            );
          }
        }
      }
    }
  }

  pub async fn contains_group(&self, object_id: &str) -> bool {
    self.group_by_object_id.get(object_id).is_some()
  }

  pub async fn get_group(&self, object_id: &str) -> Option<Rc<CollabGroup>> {
    self
      .group_by_object_id
      .get(object_id)
      .map(|v| v.value().clone())
  }

  pub fn get_group_if(
    &self,
    object_id: &str,
    f: impl FnOnce(&CollabGroup) -> bool,
  ) -> Option<Rc<CollabGroup>> {
    self
      .group_by_object_id
      .get(object_id)
      .filter(|v| f(v.value()))
      .map(|v| v.value().clone())
  }

  pub fn record_editing(&self, user: &RealtimeUser, editing: Editing) {
    self
      .editing_by_user
      .entry(user.clone())
      .or_default()
      .insert(editing);
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

  pub async fn subscribe_group(
    &self,
    user: &RealtimeUser,
    object_id: &str,
    origin: &CollabOrigin,
    client_msg_router: &mut ClientMessageRouter,
  ) -> Result<(), RealtimeError> {
    if let Some(collab_group) = self.get_group_if(object_id, |group| !group.contains_user(user)) {
      trace!("[realtime]: {} subscribe group:{}", user, object_id,);
      self.record_editing(
        user,
        Editing {
          object_id: object_id.to_string(),
          origin: origin.clone(),
        },
      );

      let (sink, stream) = client_msg_router.init_client_communication::<CollabMessage, _>(
        &collab_group.workspace_id,
        user,
        object_id,
        self.access_control.clone(),
      );

      collab_group
        .subscribe(user, origin.clone(), sink, stream)
        .await;
      Ok(())
    } else {
      // When subscribing to a group, the group should exist. Otherwise, it's a bug.
      Err(RealtimeError::Internal(anyhow!(
        "The group:{} is not found",
        object_id
      )))
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
    let group = Rc::new(
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

#[cfg(test)]
mod tests {

  struct MockStorage;
  #[tokio::test]
  async fn create_group_test() {}
}
