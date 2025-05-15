use std::sync::Arc;
use std::time::Duration;

use access_control::collab::RealtimeAccessControl;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::CollabMessage;
use collab_stream::client::CollabRedisStream;
use database::collab::CollabStorage;
use database_entity::dto::QueryCollabParams;
use tracing::trace;
use uuid::Uuid;
use yrs::{ReadTxn, StateVector};

use crate::client::client_msg_router::ClientMessageRouter;
use crate::error::RealtimeError;
use crate::group::group_init::CollabGroup;
use crate::group::state::GroupManagementState;
use crate::metrics::CollabRealtimeMetrics;
use indexer::scheduler::IndexerScheduler;

pub struct GroupManager<S> {
  state: GroupManagementState,
  storage: Arc<S>,
  access_control: Arc<dyn RealtimeAccessControl>,
  metrics_calculate: Arc<CollabRealtimeMetrics>,
  collab_redis_stream: Arc<CollabRedisStream>,
  persistence_interval: Duration,
  prune_grace_period: Duration,
  indexer_scheduler: Arc<IndexerScheduler>,
}

impl<S> GroupManager<S>
where
  S: CollabStorage,
{
  #[allow(clippy::too_many_arguments)]
  pub async fn new(
    storage: Arc<S>,
    access_control: Arc<dyn RealtimeAccessControl>,
    metrics_calculate: Arc<CollabRealtimeMetrics>,
    collab_stream: CollabRedisStream,
    persistence_interval: Duration,
    prune_grace_period: Duration,
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
      prune_grace_period,
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
    let params = QueryCollabParams::new(object_id, collab_type, workspace_id);
    let res = self.storage.get_encode_collab(params, false).await;
    let state_vector = match res {
      Ok(collab) => Collab::new_with_source(
        CollabOrigin::Server,
        &object_id.to_string(),
        DataSource::DocStateV1(collab.doc_state.into()),
        vec![],
        false,
      )?
      .transact()
      .state_vector(),
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
      self.prune_grace_period,
      state_vector,
      self.indexer_scheduler.clone(),
    )
    .await?;
    self.state.insert_group(object_id, group);
    Ok(())
  }
}
