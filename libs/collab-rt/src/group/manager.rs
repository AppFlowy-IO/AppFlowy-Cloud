use crate::client_msg_router::ClientMessageRouter;
use crate::group::group_init::{CollabGroup, MutexCollab};
use crate::group::state::GroupManagementState;

use crate::error::RealtimeError;
use crate::metrics::CollabMetricsCalculate;
use crate::RealtimeAccessControl;
use app_error::AppError;
use collab::core::collab::DocStateSource;
use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;

use crate::group::plugin::HistoryPlugin;
use collab::preclude::{Collab, CollabPlugin};
use collab_entity::CollabType;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::CollabMessage;
use database::collab::CollabStorage;
use database_entity::dto::QueryCollabParams;
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

  pub async fn get_group(&self, object_id: &str) -> Option<Arc<CollabGroup>> {
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
  ) -> Result<(), RealtimeError> {
    let mut is_new_collab = false;
    let params = QueryCollabParams::new(object_id, collab_type.clone(), workspace_id);
    let result = load_collab(uid, object_id, params, self.storage.clone()).await;
    let mutex_collab = {
      let collab = match result {
        Ok(collab) => collab,
        Err(err) => {
          if err.is_record_not_found() {
            is_new_collab = true;
            MutexCollab::new(Collab::new_with_origin(
              CollabOrigin::Server,
              object_id,
              vec![],
              false,
            ))
          } else {
            return Err(RealtimeError::Internal(err.into()));
          }
        },
      };

      let plugins: Vec<Box<dyn CollabPlugin>> = vec![Box::new(HistoryPlugin::new(
        workspace_id.to_string(),
        object_id.to_string(),
        collab_type.clone(),
        collab.downgrade(),
        self.storage.clone(),
        is_new_collab,
      ))];

      collab.lock().add_plugins(plugins);
      collab.lock().initialize();
      collab
    };

    debug!(
      "[realtime]: {} create group:{}:{}",
      uid, object_id, collab_type
    );
    let group = Arc::new(
      CollabGroup::new(
        uid,
        workspace_id.to_string(),
        object_id.to_string(),
        collab_type,
        mutex_collab,
        self.metrics_calculate.clone(),
        self.storage.clone(),
        is_new_collab,
      )
      .await,
    );
    self.state.insert_group(object_id, group.clone()).await;
    Ok(())
  }
}

async fn load_collab<S>(
  uid: i64,
  object_id: &str,
  params: QueryCollabParams,
  storage: Arc<S>,
) -> Result<MutexCollab, AppError>
where
  S: CollabStorage,
{
  let encode_collab = storage
    .get_collab_encoded(&uid, params.clone(), true)
    .await?;
  let result = Collab::new_with_doc_state(
    CollabOrigin::Server,
    object_id,
    DocStateSource::FromDocState(encode_collab.doc_state.to_vec()),
    vec![],
    false,
  )
  .map(MutexCollab::new);
  match result {
    Ok(collab) => Ok(collab),
    Err(err) => load_collab_from_snapshot(object_id, params, storage)
      .await
      .ok_or_else(|| AppError::Internal(err.into())),
  }
}

async fn load_collab_from_snapshot<S>(
  object_id: &str,
  params: QueryCollabParams,
  storage: Arc<S>,
) -> Option<MutexCollab>
where
  S: CollabStorage,
{
  let encode_collab = get_latest_snapshot(
    &params.workspace_id,
    object_id,
    &storage,
    &params.collab_type,
  )
  .await?;
  let collab = Collab::new_with_doc_state(
    CollabOrigin::Server,
    object_id,
    DocStateSource::FromDocState(encode_collab.doc_state.to_vec()),
    vec![],
    false,
  )
  .ok()?;
  Some(MutexCollab::new(collab))
}

async fn get_latest_snapshot<S>(
  workspace_id: &str,
  object_id: &str,
  storage: &S,
  collab_type: &CollabType,
) -> Option<EncodedCollab>
where
  S: CollabStorage,
{
  let metas = storage.get_collab_snapshot_list(object_id).await.ok()?.0;
  for meta in metas {
    let snapshot_data = storage
      .get_collab_snapshot(workspace_id, &meta.object_id, &meta.snapshot_id)
      .await
      .ok()?;
    if let Ok(encoded_collab) = EncodedCollab::decode_from_bytes(&snapshot_data.encoded_collab_v1) {
      if let Ok(collab) = Collab::new_with_doc_state(
        CollabOrigin::Empty,
        object_id,
        DocStateSource::FromDocState(encoded_collab.doc_state.to_vec()),
        vec![],
        false,
      ) {
        // TODO(nathan): this check is not necessary, can be removed in the future.
        collab_type.validate(&collab).ok()?;
        return Some(encoded_collab);
      }
    }
  }
  None
}
