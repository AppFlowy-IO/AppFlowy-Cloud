use crate::client::client_msg_router::ClientMessageRouter;
use crate::error::{CreateGroupFailedReason, RealtimeError};
use crate::group::collab_group::CollabGroup;
use crate::group::database_init::DatabaseCollabGroup;
use crate::group::group_init::DefaultCollabGroup;
use crate::group::persister::CollabPersister;
use crate::group::state::GroupManagementState;
use crate::indexer::IndexerProvider;
use crate::metrics::CollabRealtimeMetrics;
use access_control::collab::RealtimeAccessControl;
use app_error::AppError;
use bytes::Bytes;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::error::CollabError;
use collab::preclude::Collab;
use collab_entity::define::DATABASE_ID;
use collab_entity::CollabType;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::CollabMessage;
use collab_rt_protocol::{Message, MessageReader, SyncMessage};
use collab_stream::client::CollabRedisStream;
use database::collab::{CollabStorage, GetCollabOrigin};
use database_entity::dto::QueryCollabParams;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, instrument, trace};
use uuid::Uuid;
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::{Map, Update};

pub struct GroupManager<S> {
  state: GroupManagementState,
  storage: Arc<S>,
  access_control: Arc<dyn RealtimeAccessControl>,
  metrics_calculate: Arc<CollabRealtimeMetrics>,
  collab_redis_stream: Arc<CollabRedisStream>,
  persistence_interval: Duration,
  prune_grace_period: Duration,
  indexer_provider: Arc<IndexerProvider>,
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
    indexer_provider: Arc<IndexerProvider>,
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
      indexer_provider,
    })
  }

  pub async fn get_inactive_groups(&self) -> Vec<String> {
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

  pub async fn get_group(&self, object_id: &str) -> Option<CollabGroup> {
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
    if let Some(mut e) = self.state.get_mut_group(object_id).await {
      let group = e.value_mut();
      trace!("[realtime]: {} subscribe group:{}", user, object_id);
      let (sink, stream) = client_msg_router.init_client_communication::<CollabMessage>(
        group.workspace_id(),
        user,
        object_id,
        self.access_control.clone(),
      );
      group
        .subscribe(user, message_origin.clone(), sink, stream)
        .await;
      // explicitly drop the group to release the lock.
      drop(e);

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
    payload: &[u8],
  ) -> Result<(), RealtimeError> {
    let mut is_new_collab = false;
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

    trace!(
      "[realtime]: create group: uid:{},workspace_id:{},object_id:{}:{}",
      user.uid,
      workspace_id,
      object_id,
      collab_type
    );

    let mut indexer = self.indexer_provider.indexer_for(collab_type.clone());
    if indexer.is_some()
      && !self
        .indexer_provider
        .can_index_workspace(workspace_id)
        .await
        .map_err(|e| RealtimeError::Internal(e.into()))?
    {
      tracing::trace!("workspace {} indexing is disabled", workspace_id);
      indexer = None;
    }
    let persister = CollabPersister::new(
      user.uid,
      self.storage.clone(),
      self.collab_redis_stream.clone(),
      indexer,
      self.metrics_calculate.clone(),
      self.prune_grace_period,
    );
    let group = match &collab_type {
      CollabType::Database | CollabType::DatabaseRow => {
        let database_id = self
          .get_database_id(workspace_id, object_id, collab_type.clone(), payload)
          .await?;
        let database_id = match database_id {
          Some(id) => id,
          None => {
            tracing::warn!(
              "couldn't find database_id for {} {}",
              collab_type,
              object_id
            );
            return Err(RealtimeError::GroupNotFound(object_id.into()));
          },
        };
        let database_group = self.get_database_group(database_id).await?;
        CollabGroup::Database(database_group.scoped(object_id.into()))
      },
      _ => {
        let default_group = DefaultCollabGroup::new(
          workspace_id.to_string(),
          object_id.to_string(),
          collab_type,
          self.metrics_calculate.clone(),
          persister,
          is_new_collab,
          self.persistence_interval,
        )
        .await?;
        CollabGroup::Default(default_group)
      },
    };
    self.state.insert_group(object_id, group).await;
    Ok(())
  }

  async fn get_database_id(
    &self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
    payload: &[u8],
  ) -> Result<Option<Uuid>, RealtimeError> {
    let mut collab = Collab::new(0, object_id, "", vec![], false);
    Self::apply_sync_messages(&mut collab, payload)?;

    tracing::debug!("couldn't resolve database_id out of collab {}", object_id);

    let params = QueryCollabParams::new(object_id, collab_type, workspace_id);
    let encode_collab = self
      .storage
      .get_encode_collab(GetCollabOrigin::Server, params, false)
      .await
      .map_err(|e| RealtimeError::Internal(e.into()))?;

    let update = Update::decode_v1(&encode_collab.doc_state)?;
    collab
      .transact_mut()
      .apply_update(update)
      .map_err(|e| RealtimeError::CollabError(CollabError::Internal(e.into())))?;
    let db_id: Option<String> = collab.data.get_as(&collab.transact(), DATABASE_ID)?;
    if let Some(id) = db_id {
      let uuid = Uuid::parse_str(id.as_str()).map_err(|e| RealtimeError::Internal(e.into()))?;
      return Ok(Some(uuid));
    }
    Ok(None)
  }

  fn apply_sync_messages(collab: &mut Collab, payload: &[u8]) -> Result<(), RealtimeError> {
    let mut decoder = DecoderV1::from(payload);
    let reader = MessageReader::new(&mut decoder);
    let mut txn = collab.transact_mut();
    for msg in reader {
      match msg? {
        Message::Sync(SyncMessage::SyncStep2(update))
        | Message::Sync(SyncMessage::Update(update)) => {
          let u = yrs::Update::decode_v1(&update)?;
          txn
            .apply_update(u)
            .map_err(|e| RealtimeError::CollabError(CollabError::Internal(e.into())))?;
        },
        _ => {},
      }
    }
    Ok(())
  }

  async fn get_database_group(
    &self,
    database_id: Uuid,
  ) -> Result<DatabaseCollabGroup, RealtimeError> {
    let object_id = database_id.to_string();
    if let Some(CollabGroup::Database(group)) = self.state.get_group(&object_id).await {
      Ok(group)
    } else {
      todo!()
    }
  }
}
