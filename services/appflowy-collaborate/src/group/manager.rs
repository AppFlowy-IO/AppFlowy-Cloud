use std::sync::Arc;
use std::time::Duration;

use access_control::collab::RealtimeAccessControl;
use collab::core::collab::{default_client_id, CollabOptions, DataSource};
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::CollabMessage;
use collab_stream::client::CollabRedisStream;
use database::collab::{CollabStore, GetCollabOrigin};
use tracing::trace;
use uuid::Uuid;
use yrs::{ReadTxn, StateVector};

use crate::client::client_msg_router::ClientMessageRouter;
use crate::error::RealtimeError;
use crate::group::group_init::CollabGroup;
use crate::group::state::GroupManagementState;
use crate::metrics::CollabRealtimeMetrics;
use indexer::scheduler::IndexerScheduler;

pub struct GroupManager {
  state: GroupManagementState,
  storage: Arc<dyn CollabStore>,
  access_control: Arc<dyn RealtimeAccessControl>,
  metrics_calculate: Arc<CollabRealtimeMetrics>,
  collab_redis_stream: Arc<CollabRedisStream>,
  persistence_interval: Duration,
  indexer_scheduler: Arc<IndexerScheduler>,
}

impl GroupManager {
  #[allow(clippy::too_many_arguments)]
  pub async fn new(
    storage: Arc<dyn CollabStore>,
    access_control: Arc<dyn RealtimeAccessControl>,
    metrics_calculate: Arc<CollabRealtimeMetrics>,
    collab_stream: CollabRedisStream,
    persistence_interval: Duration,
    indexer_scheduler: Arc<IndexerScheduler>,
  ) -> Result<Self, RealtimeError> {
    let collab_stream = Arc::new(collab_stream);
    Ok(Self {
      state: GroupManagementState::new(metrics_calculate.clone()),
      storage,
      access_control,
      metrics_calculate,
      collab_redis_stream: collab_stream,
      persistence_interval,
      indexer_scheduler,
    })
  }

  pub fn get_inactive_groups(&self) -> Vec<Uuid> {
    self.state.remove_inactive_groups()
  }

  pub fn contains_user(&self, object_id: &Uuid, user: &RealtimeUser) -> bool {
    self.state.contains_user(object_id, user)
  }

  pub fn remove_user(&self, user: &RealtimeUser) {
    self.state.remove_user(user);
  }

  pub fn contains_group(&self, object_id: &Uuid) -> bool {
    self.state.contains_group(object_id)
  }

  pub async fn get_group(&self, object_id: &Uuid) -> Option<Arc<CollabGroup>> {
    self.state.get_group(object_id).await
  }

  pub async fn subscribe_group(
    &self,
    user: &RealtimeUser,
    object_id: Uuid,
    message_origin: &CollabOrigin,
    client_msg_router: &mut ClientMessageRouter,
  ) -> Result<(), RealtimeError> {
    // Lock the group and subscribe the user to the group.
    if let Some(mut e) = self.state.get_mut_group(&object_id).await {
      let group = e.value_mut();
      trace!("[realtime]: {} subscribe group:{}", user, object_id,);
      let (sink, stream) = client_msg_router.init_client_communication::<CollabMessage>(
        *group.workspace_id(),
        user,
        object_id,
        self.access_control.clone(),
      );
      group.subscribe(user, message_origin.clone(), sink, stream);
      // explicitly drop the group to release the lock.
      drop(e);

      self.state.insert_user(user, object_id)?;
    } else {
      // When subscribing to a group, the group should exist. Otherwise, it's a bug.
      return Err(RealtimeError::GroupNotFound(object_id.to_string()));
    }

    Ok(())
  }

  pub async fn create_group(
    &self,
    user: &RealtimeUser,
    workspace_id: Uuid,
    object_id: Uuid,
    collab_type: CollabType,
  ) -> Result<(), RealtimeError> {
    let res = self
      .storage
      .get_full_encode_collab(
        GetCollabOrigin::Server,
        &workspace_id,
        &object_id,
        collab_type,
      )
      .await;
    let state_vector = match res {
      Ok(value) => {
        let options = CollabOptions::new(object_id.to_string(), default_client_id())
          .with_data_source(DataSource::DocStateV1(
            value.encoded_collab.doc_state.into(),
          ));
        Collab::new_with_options(CollabOrigin::Server, options)?
          .transact()
          .state_vector()
      },
      Err(err) if err.is_record_not_found() => StateVector::default(),
      Err(err) => return Err(RealtimeError::CannotCreateGroup(err.to_string())),
    };

    trace!(
      "[realtime]: create group: uid:{},workspace_id:{},object_id:{}:{}",
      user.uid,
      workspace_id,
      object_id,
      collab_type
    );

    let group = CollabGroup::new(
      user.uid,
      workspace_id,
      object_id,
      collab_type,
      self.metrics_calculate.clone(),
      self.storage.clone(),
      self.collab_redis_stream.clone(),
      self.persistence_interval,
      state_vector,
      self.indexer_scheduler.clone(),
    )
    .await?;
    self.state.insert_group(object_id, group);
    Ok(())
  }
}
