use std::sync::Arc;

use collab::core::collab::{DataSource, MutexCollab};
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::{Collab, CollabPlugin};
use collab_entity::CollabType;
use tokio::sync::Mutex;
use tracing::{error, instrument, trace};

use access_control::collab::RealtimeAccessControl;
use app_error::AppError;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::CollabMessage;
use collab_stream::client::{CollabRedisStream, CONTROL_STREAM_KEY};
use collab_stream::model::CollabControlEvent;
use collab_stream::stream_group::StreamGroup;
use database::collab::CollabStorage;
use database_entity::dto::QueryCollabParams;

use crate::client::client_msg_router::ClientMessageRouter;
use crate::error::{CreateGroupFailedReason, RealtimeError};
use crate::group::group_init::CollabGroup;
use crate::group::plugin::HistoryPlugin;
use crate::group::state::GroupManagementState;
use crate::metrics::CollabMetricsCalculate;

pub struct GroupManager<S, AC> {
  state: GroupManagementState,
  storage: Arc<S>,
  access_control: Arc<AC>,
  metrics_calculate: CollabMetricsCalculate,
  collab_redis_stream: Arc<CollabRedisStream>,
  control_event_stream: Arc<Mutex<StreamGroup>>,
}

impl<S, AC> GroupManager<S, AC>
where
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  pub async fn new(
    storage: Arc<S>,
    access_control: Arc<AC>,
    metrics_calculate: CollabMetricsCalculate,
    collab_stream: CollabRedisStream,
  ) -> Result<Self, RealtimeError> {
    let collab_stream = Arc::new(collab_stream);
    let control_event_stream = collab_stream
      .collab_control_stream(CONTROL_STREAM_KEY, "collaboration")
      .await
      .map_err(|err| RealtimeError::Internal(err.into()))?;
    let control_event_stream = Arc::new(Mutex::new(control_event_stream));
    Ok(Self {
      state: GroupManagementState::new(metrics_calculate.clone()),
      storage,
      access_control,
      metrics_calculate,
      collab_redis_stream: collab_stream,
      control_event_stream,
    })
  }

  pub async fn inactive_groups(&self) -> Vec<String> {
    self.state.get_inactive_group_ids().await
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

    let close_event = CollabControlEvent::Close {
      object_id: object_id.to_string(),
    };
    if let Err(err) = self
      .control_event_stream
      .lock()
      .await
      .insert_message(close_event)
      .await
    {
      error!("Failed to insert close event to control stream: {}", err);
    }
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
    user: &RealtimeUser,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Result<(), RealtimeError> {
    let mut is_new_collab = false;
    let params = QueryCollabParams::new(object_id, collab_type.clone(), workspace_id);
    // Ensure the workspace_id matches the metadata's workspace_id when creating a collaboration object
    // of type [CollabType::Folder]. In this case, both the object id and the workspace id should be
    // identical.
    if let Ok(metadata) = self
      .storage
      .query_collab_meta(object_id, &collab_type)
      .await
    {
      if metadata.workspace_id != workspace_id {
        let err =
          RealtimeError::CreateGroupFailed(CreateGroupFailedReason::CollabWorkspaceIdNotMatch {
            expect: metadata.workspace_id,
            actual: workspace_id.to_string(),
            detail: format!(
              "user_id:{},app_version:{},object_id:{}:{}",
              user.uid, user.app_version, object_id, collab_type
            ),
          });
        return Err(err);
      }
    }

    let result = load_collab(user.uid, object_id, params, self.storage.clone()).await;
    let (mutex_collab, encode_collab) = {
      let (mutex_collab, encode_collab) = match result {
        Ok(value) => value,
        Err(err) => {
          if err.is_record_not_found() {
            is_new_collab = true;
            let mutex_collab = MutexCollab::new(Collab::new_with_origin(
              CollabOrigin::Server,
              object_id,
              vec![],
              false,
            ));
            let encode_collab = mutex_collab
              .lock()
              .encode_collab_v1(|_| Ok::<_, RealtimeError>(()))?;
            (mutex_collab, encode_collab)
          } else {
            return Err(RealtimeError::CreateGroupFailed(
              CreateGroupFailedReason::CannotGetCollabData,
            ));
          }
        },
      };

      let plugins: Vec<Box<dyn CollabPlugin>> = vec![Box::new(HistoryPlugin::new(
        workspace_id.to_string(),
        object_id.to_string(),
        collab_type.clone(),
        mutex_collab.downgrade(),
        self.storage.clone(),
        is_new_collab,
      ))];

      mutex_collab.lock().add_plugins(plugins);
      mutex_collab.lock().initialize();
      (mutex_collab, encode_collab)
    };

    let cloned_control_event_stream = self.control_event_stream.clone();
    let open_event = CollabControlEvent::Open {
      workspace_id: workspace_id.to_string(),
      object_id: object_id.to_string(),
      collab_type: collab_type.clone(),
      doc_state: encode_collab.doc_state.to_vec(),
    };
    tokio::spawn(async move {
      if let Err(err) = cloned_control_event_stream
        .lock()
        .await
        .insert_message(open_event)
        .await
      {
        error!("Failed to insert open event to control stream: {}", err);
      }
    });

    trace!(
      "[realtime]: create group: uid:{},workspace_id:{},object_id:{}:{}",
      user.uid,
      workspace_id,
      object_id,
      collab_type
    );

    let group = Arc::new(
      CollabGroup::new(
        user.uid,
        workspace_id.to_string(),
        object_id.to_string(),
        collab_type,
        mutex_collab,
        self.metrics_calculate.clone(),
        self.storage.clone(),
        is_new_collab,
        self.collab_redis_stream.clone(),
      )
      .await?,
    );
    self.state.insert_group(object_id, group.clone()).await;
    Ok(())
  }
}

#[instrument(level = "trace", skip_all)]
async fn load_collab<S>(
  uid: i64,
  object_id: &str,
  params: QueryCollabParams,
  storage: Arc<S>,
) -> Result<(MutexCollab, EncodedCollab), AppError>
where
  S: CollabStorage,
{
  let encode_collab = storage
    .get_encode_collab(&uid, params.clone(), true)
    .await?;
  let result = Collab::new_with_source(
    CollabOrigin::Server,
    object_id,
    DataSource::DocStateV1(encode_collab.doc_state.to_vec()),
    vec![],
    false,
  )
  .map(MutexCollab::new);
  match result {
    Ok(collab) => Ok((collab, encode_collab)),
    Err(err) => load_collab_from_snapshot(object_id, params, storage)
      .await
      .ok_or_else(|| AppError::Internal(err.into())),
  }
}

async fn load_collab_from_snapshot<S>(
  object_id: &str,
  params: QueryCollabParams,
  storage: Arc<S>,
) -> Option<(MutexCollab, EncodedCollab)>
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
  let collab = Collab::new_with_source(
    CollabOrigin::Server,
    object_id,
    DataSource::DocStateV1(encode_collab.doc_state.to_vec()),
    vec![],
    false,
  )
  .ok()?;
  Some((MutexCollab::new(collab), encode_collab))
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
      if let Ok(collab) = Collab::new_with_source(
        CollabOrigin::Empty,
        object_id,
        DataSource::DocStateV1(encoded_collab.doc_state.to_vec()),
        vec![],
        false,
      ) {
        // TODO(nathan): this check is not necessary, can be removed in the future.
        collab_type.validate_require_data(&collab).ok()?;
        return Some(encoded_collab);
      }
    }
  }
  None
}
