use crate::client_msg_router::ClientMessageRouter;
use crate::collaborate::group::CollabGroup;
use crate::collaborate::group_manager_state::GroupManagementState;
use crate::collaborate::plugin::LoadCollabPlugin;
use crate::error::RealtimeError;
use crate::RealtimeAccessControl;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::CollabMessage;
use std::rc::Rc;

use database::collab::CollabStorage;

use crate::metrics::CollabMetricsCalculate;

use std::sync::Arc;
use tracing::{debug, instrument, trace};

pub struct GroupManager<S, AC> {
  state: GroupManagementState,
  storage: Arc<S>,
  access_control: Arc<AC>,
  metrics_calculate: CollabMetricsCalculate,
}

impl<S, AC> GroupManager<S, AC>
where
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  pub fn new(
    storage: Arc<S>,
    access_control: Arc<AC>,
    metrics_calculate: CollabMetricsCalculate,
  ) -> Self {
    Self {
      state: GroupManagementState::new(metrics_calculate.clone()),
      storage,
      access_control,
      metrics_calculate,
    }
  }

  pub async fn inactive_groups(&self) -> Vec<String> {
    self.state.tick().await
  }

  pub async fn contains_user(&self, object_id: &str, user: &RealtimeUser) -> bool {
    self.state.contains_user(object_id, user).await
  }

  pub async fn remove_user(&self, user: &RealtimeUser) {
    self.state.remove_user(user).await;
  }

  pub async fn contains_group(&self, object_id: &str) -> bool {
    self.state.contains_group(object_id).await
  }

  pub async fn get_group(&self, object_id: &str) -> Option<Rc<CollabGroup>> {
    self.state.get_group(object_id).await
  }

  #[instrument(skip(self))]
  async fn remove_group(&self, object_id: &str) {
    self.state.remove_group(object_id).await;
  }

  pub async fn subscribe_group(
    &self,
    user: &RealtimeUser,
    object_id: &str,
    message_origin: &CollabOrigin,
    client_msg_router: &mut ClientMessageRouter,
  ) -> Result<(), RealtimeError> {
    // Lock the group and subscribe the user to the group.
    if let Some(group) = self.state.get_mut_group(object_id).await {
      trace!("[realtime]: {} subscribe group:{}", user, object_id,);
      let (sink, stream) = client_msg_router.init_client_communication::<CollabMessage, _>(
        &group.workspace_id,
        user,
        object_id,
        self.access_control.clone(),
      );
      group
        .subscribe(user, message_origin.clone(), sink, stream)
        .await;
      // explicitly drop the group to release the lock.
      drop(group);

      self.state.insert_user(user, object_id).await?;
    } else {
      // When subscribing to a group, the group should exist. Otherwise, it's a bug.
      return Err(RealtimeError::GroupNotFound(object_id.to_string()));
    }

    Ok(())
  }

  pub async fn create_group(
    &self,
    uid: i64,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) {
    let mut collab = Collab::new_with_origin(CollabOrigin::Server, object_id, vec![], false);
    let plugin = LoadCollabPlugin::new(
      uid,
      workspace_id,
      collab_type.clone(),
      self.storage.clone(),
      self.access_control.clone(),
    );
    collab.add_plugin(Box::new(plugin));
    collab.initialize().await;

    // The lifecycle of the collab is managed by the group.
    debug!(
      "[realtime]: {} create group:{}:{}",
      uid, object_id, collab_type
    );
    let group = Rc::new(
      CollabGroup::new(
        uid,
        workspace_id.to_string(),
        object_id.to_string(),
        collab_type,
        collab,
        self.metrics_calculate.clone(),
        self.storage.clone(),
      )
      .await,
    );
    self.state.insert_group(object_id, group.clone()).await;
  }
}
