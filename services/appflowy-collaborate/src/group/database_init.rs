use crate::error::RealtimeError;
use crate::group::collab_group::{CollabGroup, SubscriptionSink, SubscriptionStream};
use crate::group::persister::CollabPersister;
use crate::CollabRealtimeMetrics;
use anyhow::anyhow;
use arc_swap::ArcSwap;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab_database::database::Database;
use collab_entity::CollabType;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::{
  AckCode, AwarenessSync, BroadcastSync, ClientCollabMessage, CollabAck, MessageByObjectId, MsgId,
};
use collab_rt_protocol::{Message, MessageReader, RTProtocolError, SyncMessage};
use collab_stream::collab_update_sink::{AwarenessUpdateSink, CollabUpdateSink};
use collab_stream::model::{AwarenessStreamUpdate, CollabStreamUpdate, UpdateFlags};
use dashmap::DashMap;
use futures::pin_mut;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{ReadTxn, StateVector, Update};

//TODO: this should be defined at collab crate level.
pub type WorkspaceId = Arc<str>;
//TODO: this should be defined at collab crate level.
pub type ObjectId = Arc<str>;

#[derive(Clone)]
pub struct DatabaseCollabGroup {
  object_id: Option<ObjectId>,
  state: Arc<Inner>,
}

impl DatabaseCollabGroup {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    workspace_id: WorkspaceId,
    database_id: ObjectId,
    persister: CollabPersister,
    metrics: Arc<CollabRealtimeMetrics>,
    persistence_interval: Duration,
  ) -> Self {
    let state = Arc::new(Inner::new(workspace_id, database_id, persister, metrics));

    // setup task used to receive collab updates from Redis
    {
      let state = state.clone();
      tokio::spawn(async move {
        if let Err(err) = Self::inbound_task(state).await {
          tracing::warn!("failed to receive collab update: {}", err);
        }
      });
    }

    // setup task used to receive awareness updates from Redis
    {
      let state = state.clone();
      tokio::spawn(async move {
        if let Err(err) = Self::inbound_awareness_task(state).await {
          tracing::warn!("failed to receive awareness update: {}", err);
        }
      });
    }

    // setup periodic snapshot
    {
      tokio::spawn(Self::snapshot_task(state.clone(), persistence_interval));
    }
  }

  /// Reinterprets commands performed on the underlying group as made from the perspective of
  /// given `object_id`.
  pub fn scoped(self, object_id: ObjectId) -> Self {
    if &*self.object_id == Some(&object_id) {
      self
    } else {
      DatabaseCollabGroup {
        object_id: Some(object_id),
        state: self.state,
      }
    }
  }

  pub fn is_database(&self) -> bool {
    self.object_id.is_none()
  }

  pub fn collab_type(&self) -> CollabType {
    if self.is_database() {
      CollabType::Database
    } else {
      CollabType::DatabaseRow
    }
  }

  pub async fn encode_collab(&self) -> Result<EncodedCollab, RealtimeError> {
    let collab_type = self.collab_type();
    let snapshot = self
      .state
      .persister
      .load_compact(
        &self.state.workspace_id,
        &self.object_id.to_string(),
        collab_type.clone(),
      )
      .await?;
    let encode_collab = snapshot.collab.encode_collab_v1(|collab| {
      collab_type
        .validate_require_data(collab)
        .map_err(|err| RealtimeError::Internal(err.into()))
    })?;
    Ok(encode_collab)
  }

  pub async fn subscribe<Sink, Stream>(
    &self,
    user: &RealtimeUser,
    subscriber_origin: CollabOrigin,
    sink: Sink,
    stream: Stream,
  ) where
    Sink: SubscriptionSink + Clone + 'static,
    Stream: SubscriptionStream + 'static,
  {
    // create new subscription for new subscriber
    let subscriber_shutdown = self.state.shutdown.child_token();

    tokio::spawn(Self::receive_from_client_task(
      user.clone(),
      self.object_id.clone(),
      self.state.clone(),
      sink.clone(),
      stream,
      subscriber_origin.clone(),
    ));

    let sub = Subscription::new(sink, subscriber_origin, subscriber_shutdown);
    let group_id = self.object_id.as_ref().unwrap_or(&self.state.database_id);
    let subgroup = self.state.subgroups.entry(group_id.clone()).or_default();
    let subgroup = subgroup.write().await;
    if subgroup.subscribers.insert(user.clone(), sub).is_some() {
      tracing::warn!("{}: removed old subscriber: {}", group_id, user);
    }

    if cfg!(debug_assertions) {
      tracing::trace!("{}: added new subscriber", group_id);
    }
  }

  async fn receive_from_client_task<Sink, Stream>(
    user: RealtimeUser,
    row_id: Option<ObjectId>,
    state: Arc<Inner>,
    mut sink: Sink,
    mut stream: Stream,
    origin: CollabOrigin,
  ) where
    Sink: SubscriptionSink + 'static,
    Stream: SubscriptionStream + 'static,
  {
    loop {
      tokio::select! {
        _ = state.shutdown.cancelled() => {
          break;
        }
        msg = stream.next() => {
          match msg {
            None => break,
            Some(msg) => if let Err(err) =  Self::handle_messages(&user, &state, &mut sink, msg, row_id.as_ref()).await {
              tracing::warn!(
                "collab `{}` failed to handle message from `{}`: {}",
                row_id,
                origin,
                err
              );

            }
          }
        }
      }
    }
  }

  async fn handle_messages<Sink>(
    user: &RealtimeUser,
    state: &Inner,
    sink: &mut Sink,
    msg: MessageByObjectId,
    row_id: Option<&ObjectId>,
  ) -> Result<(), RealtimeError>
  where
    Sink: SubscriptionSink + 'static,
  {
    for (object_id, messages) in msg {
      if let Some(subgroup) = state.subgroups.get(object_id.as_str()) {
        let subgroup = subgroup.read().await;
        for message in messages {
          match Self::handle_client_message(user, &object_id, &subgroup, state, message).await {
            Ok(response) => {
              tracing::trace!("[realtime]: sending response: {}", response);
              match sink.send(response.into()).await {
                Ok(()) => {},
                Err(err) => {
                  tracing::trace!("[realtime]: send failed: {}", err);
                  break;
                },
              }
            },
            Err(err) => {
              tracing::error!(
                "Error handling collab message for object_id: {}: {}",
                object_id,
                err
              );
              break;
            },
          }
        }
      }
    }
    Ok(())
  }

  /// Handle the message sent from the client
  async fn handle_client_message(
    user: &RealtimeUser,
    object_id: &str,
    subgroup: &Subgroup,
    state: &Inner,
    collab_msg: ClientCollabMessage,
  ) -> Result<CollabAck, RealtimeError> {
    let msg_id = collab_msg.msg_id();
    let message_origin = collab_msg.origin().clone();

    let subscription = subgroup
      .subscribers
      .get(user)
      .ok_or_else(|| RealtimeError::UserNotFound(user.to_string()))?;
    // If the payload is empty, we don't need to apply any updates .
    // Currently, only the ping message should has an empty payload.
    if collab_msg.payload().is_empty() {
      if !matches!(collab_msg, ClientCollabMessage::ClientCollabStateCheck(_)) {
        tracing::error!("receive unexpected empty payload message:{}", collab_msg);
      }
      return Ok(CollabAck::new(
        message_origin,
        object_id.to_string(),
        msg_id,
        subgroup.seq_no.load(Ordering::SeqCst),
      ));
    }

    tracing::trace!(
      "Applying client updates: {}, origin:{}",
      collab_msg,
      message_origin
    );

    let payload = collab_msg.payload();

    // Spawn a blocking task to handle the message
    let result =
      Self::handle_message(subgroup, object_id, state, payload, &message_origin, msg_id).await;

    match result {
      Ok(inner_result) => match inner_result {
        Some(response) => Ok(response),
        None => Err(RealtimeError::UnexpectedData("No ack response")),
      },
      Err(err) => Err(RealtimeError::Internal(anyhow!(
        "fail to handle message:{}",
        err
      ))),
    }
  }

  async fn handle_message(
    subgroup: &Subgroup,
    object_id: &str,
    state: &Inner,
    payload: &[u8],
    message_origin: &CollabOrigin,
    msg_id: MsgId,
  ) -> Result<Option<CollabAck>, RealtimeError> {
    let mut decoder = DecoderV1::from(payload);
    let reader = MessageReader::new(&mut decoder);
    let mut ack_response = None;
    for msg in reader {
      match msg {
        Ok(msg) => {
          match Self::handle_protocol_message(subgroup, state, message_origin, msg).await {
            Ok(payload) => {
              // One ClientCollabMessage can have multiple Yrs [Message] in it, but we only need to
              // send one ack back to the client.
              if ack_response.is_none() {
                ack_response = Some(
                  CollabAck::new(
                    message_origin.clone(),
                    object_id.to_string(),
                    msg_id,
                    subgroup.seq_no.load(Ordering::SeqCst),
                  )
                  .with_payload(payload.unwrap_or_default()),
                );
              }
            },
            Err(err) => {
              tracing::warn!("[realtime]: failed to handled message: {}", msg_id);
              state.metrics.apply_update_failed_count.inc();

              let code = Self::ack_code_from_error(&err);
              let payload = match err {
                RTProtocolError::MissUpdates {
                  state_vector_v1,
                  reason: _,
                } => state_vector_v1.unwrap_or_default(),
                _ => vec![],
              };

              ack_response = Some(
                CollabAck::new(
                  message_origin.clone(),
                  object_id.to_string(),
                  msg_id,
                  subgroup.seq_no.load(Ordering::SeqCst),
                )
                .with_code(code)
                .with_payload(payload),
              );

              break;
            },
          }
        },
        Err(e) => {
          tracing::error!("{} => parse sync message failed: {}", object_id, e);
          break;
        },
      }
    }
    Ok(ack_response)
  }

  async fn handle_protocol_message(
    subgroup: &Subgroup,
    state: &Inner,
    origin: &CollabOrigin,
    msg: Message,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    match msg {
      Message::Sync(msg) => match msg {
        SyncMessage::SyncStep1(sv) => Self::handle_sync_step1(subgroup, state, &sv).await,
        SyncMessage::SyncStep2(update) => {
          Self::handle_sync_step2(subgroup, state, origin, update).await
        },
        SyncMessage::Update(update) => Self::handle_update(subgroup, state, origin, update).await,
      },
      //FIXME: where is the QueryAwareness protocol?
      Message::Awareness(update) => {
        Self::handle_awareness_update(subgroup, state, origin, update).await
      },
      Message::Auth(_reason) => Ok(None),
      Message::Custom(_msg) => Ok(None),
    }
  }

  async fn handle_sync_step1(
    subgroup: &Subgroup,
    state: &Inner,
    remote_sv: &StateVector,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    let sv = &subgroup.state_vector;
    // we optimistically try to obtain state vector lock for a fast track:
    // if we remote sv is up-to-date with current one, we don't need to do anything
    match sv.partial_cmp(remote_sv) {
      Some(std::cmp::Ordering::Equal) => return Ok(None), // client and server are in sync
      Some(std::cmp::Ordering::Less) => {
        // server is behind client
        let msg = Message::Sync(SyncMessage::SyncStep1(sv.clone()));
        return Ok(Some(msg.encode_v1()));
      },
      Some(std::cmp::Ordering::Greater) | None => { /* server has some new updates */ },
    }

    // we need to reconstruct document state on the server side
    tracing::debug!("loading collab {}", state.object_id);
    let snapshot = state
      .persister
      .load_compact(
        &state.workspace_id,
        &state.object_id,
        state.collab_type.clone(),
      )
      .await
      .map_err(|err| RTProtocolError::Internal(err.into()))?;

    // prepare document state update and state vector
    let tx = snapshot.collab.transact();
    let doc_state = tx.encode_state_as_update_v1(remote_sv);
    let local_sv = tx.state_vector();
    drop(tx);

    // Retrieve the latest document state from the client after they return online from offline editing.
    tracing::trace!("sending missing data to client ({} bytes)", doc_state.len());
    let mut encoder = EncoderV1::new();
    Message::Sync(SyncMessage::SyncStep2(doc_state)).encode(&mut encoder);
    //FIXME: this should never happen as response to sync step 1 from the client, but rather be
    //  send when a connection is established
    Message::Sync(SyncMessage::SyncStep1(local_sv)).encode(&mut encoder);
    Ok(Some(encoder.to_vec()))
  }

  async fn handle_sync_step2(
    subgroup: &Subgroup,
    state: &Inner,
    origin: &CollabOrigin,
    update: Vec<u8>,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    state.metrics.collab_size.observe(update.len() as f64);

    let start = tokio::time::Instant::now();
    // we try to decode update to make sure it's not malformed and to extract state vector
    let (update, decoded_update) = if update.len() <= collab_rt_protocol::LARGE_UPDATE_THRESHOLD {
      let decoded_update = Update::decode_v1(&update)?;
      (update, decoded_update)
    } else {
      tokio::task::spawn_blocking(move || {
        let decoded_update = Update::decode_v1(&update)?;
        Ok::<(Vec<u8>, yrs::Update), yrs::encoding::read::Error>((update, decoded_update))
      })
      .await
      .map_err(|err| RTProtocolError::Internal(err.into()))??
    };
    let missing_updates = {
      let state_vector = &subgroup.state_vector;
      match state_vector.partial_cmp(&decoded_update.state_vector_lower()) {
        None | Some(std::cmp::Ordering::Less) => Some(state_vector.clone()),
        _ => None,
      }
    };

    if let Some(missing_updates) = missing_updates {
      let msg = Message::Sync(SyncMessage::SyncStep1(missing_updates));
      tracing::debug!("subscriber {} send update with missing data", origin);
      Ok(Some(msg.encode_v1()))
    } else {
      let upper_state_vector = decoded_update.state_vector();
      let msg = CollabStreamUpdate::new(
        update,
        upper_state_vector,
        origin.clone(),
        UpdateFlags::default(),
        Some(row_id),
      );
      state
        .collab_update_sink
        .send(&msg)
        .await
        .map_err(|err| RTProtocolError::Internal(err.into()))?;
      let elapsed = start.elapsed();

      state
        .metrics
        .load_collab_time
        .observe(elapsed.as_millis() as f64);

      Ok(None)
    }
  }

  async fn handle_update(
    subgroup: &Subgroup,
    state: &Inner,
    origin: &CollabOrigin,
    update: Vec<u8>,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    Self::handle_sync_step2(subgroup, state, origin, update).await
  }

  async fn handle_awareness_update(
    subgroup: &Subgroup,
    state: &Inner,
    origin: &CollabOrigin,
    data: Vec<u8>,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    let msg = AwarenessStreamUpdate {
      data,
      sender: origin.clone(),
      row: Some(row_id),
    };
    state
      .awareness_update_sink
      .send(&msg)
      .await
      .map_err(|err| RTProtocolError::Internal(err.into()))?;
    Ok(None)
  }

  #[inline]
  fn ack_code_from_error(error: &RTProtocolError) -> AckCode {
    match error {
      RTProtocolError::YrsTransaction(_) => AckCode::Retry,
      RTProtocolError::YrsApplyUpdate(_) => AckCode::CannotApplyUpdate,
      RTProtocolError::YrsEncodeState(_) => AckCode::EncodeStateAsUpdateFail,
      RTProtocolError::MissUpdates { .. } => AckCode::MissUpdate,
      _ => AckCode::Internal,
    }
  }

  pub async fn contains_user(&self, user: &RealtimeUser) -> bool {
    if let Some(subgroup) = self.state.subgroups.get(&self.object_id) {
      let subgroup = subgroup.read().await;
      subgroup.subscribers.contains_key(user)
    } else {
      false
    }
  }

  pub async fn remove_user(&self, user: &RealtimeUser) {
    if let Some(subgroup) = self.state.subgroups.get(&self.object_id) {
      let subgroup = subgroup.write().await;
      subgroup.subscribers.remove(user);
    }
  }

  /// Check if the group is active. A group is considered active if it has at least one
  /// subscriber
  pub fn is_inactive(&self) -> bool {
    self.state.shutdown.is_cancelled()
  }

  pub fn workspace_id(&self) -> &str {
    &self.state.workspace_id
  }

  /// Task used to receive collab updates from Redis.
  async fn inbound_task(state: Arc<Inner>) -> Result<(), RealtimeError> {
    let updates = state.persister.collab_redis_stream.live_collab_updates(
      &state.workspace_id,
      &state.object_id,
      None,
    );
    pin_mut!(updates);
    loop {
      tokio::select! {
        _ = state.shutdown.cancelled() => {
          break;
        }
        res = updates.next() => {
          match res {
            Some(Ok((_message_id, update))) => {
              Self::handle_inbound_update(&state, update).await;
            },
            Some(Err(err)) => {
              tracing::warn!("failed to handle incoming update for collab `{}`: {}", state.object_id, err);
              break;
            },
            None => {
              break;
            }
          }
        }
      }
    }
    Ok(())
  }

  async fn handle_inbound_update(object_id: &str, state: &Inner, update: CollabStreamUpdate) {
    if let Some(object_id) = update.row {
      if let Some(subgroup) = state.subgroups.get(&*object_id) {
        let subgroup = subgroup.write().await;
        // update state vector based on incoming message
        subgroup.state_vector.merge(update.state_vector);

        let seq_num = state.seq_no.fetch_add(1, Ordering::SeqCst) + 1;
        tracing::trace!(
          "broadcasting collab update from {} ({} bytes) - seq_num: {}",
          update.sender,
          update.data.len(),
          seq_num
        );
        let payload = Message::Sync(SyncMessage::Update(update.data)).encode_v1();
        let message = BroadcastSync::new(update.sender, object_id.into(), payload, seq_num);
        for mut e in subgroup.subscribers.iter_mut() {
          let subscription = e.value_mut();
          if message.origin == subscription.collab_origin {
            continue; // don't send update to its sender
          }

          if let Err(err) = subscription.sink.send(message.clone().into()).await {
            tracing::debug!(
              "failed to send collab `{}` update to `{}`: {}",
              object_id,
              subscription.collab_origin,
              err
            );
          }

          state.last_activity.store(Arc::new(Instant::now()));
        }
      } else {
        tracing::debug!("database collab group - no subgroup for {}", object_id);
      }
    } else {
      tracing::warn!("database collab group got awareness update without target object id");
    }
  }

  /// Task used to receive awareness updates from Redis.
  async fn inbound_awareness_task(state: Arc<Inner>) -> Result<(), RealtimeError> {
    let updates = state.persister.collab_redis_stream.awareness_updates(
      &state.workspace_id,
      &state.database_id,
      None,
    );
    pin_mut!(updates);
    loop {
      tokio::select! {
        _ = state.shutdown.cancelled() => {
          break;
        }
        res = updates.next() => {
          match res {
            Some(Ok(awareness_update)) => {
              Self::handle_inbound_awareness(&state, awareness_update).await;
            },
            Some(Err(err)) => {
              tracing::warn!("failed to handle incoming update for database: {}", err);
              break;
            },
            None => {
              break;
            }
          }
        }
      }
    }
    Ok(())
  }

  async fn handle_inbound_awareness(state: &Inner, update: AwarenessStreamUpdate) {
    tracing::trace!(
      "broadcasting awareness update from {} ({} bytes)",
      update.sender,
      update.data.len()
    );

    if let Some(object_id) = update.row {
      if let Some(subgroup) = state.subgroups.get(&*object_id) {
        let subgroup = subgroup.write().await;
        let sender = update.sender;
        let message = AwarenessSync::new(
          object_id.clone().into(),
          Message::Awareness(update.data).encode_v1(),
          CollabOrigin::Empty,
        );
        for mut e in subgroup.subscribers.iter_mut() {
          let subscription = e.value_mut();
          if sender == subscription.collab_origin {
            continue; // don't send update to its sender
          }

          if let Err(err) = subscription.sink.send(message.clone().into()).await {
            tracing::debug!(
              "failed to send awareness `{}` update to `{}`: {}",
              object_id,
              subscription.collab_origin,
              err
            );
          }

          state.last_activity.store(Arc::new(Instant::now()));
        }
      } else {
        tracing::debug!("database collab group - no subgroup for {}", object_id);
      }
    } else {
      tracing::warn!("database collab group got awareness update without target object id");
    }
  }

  async fn snapshot_task(state: Arc<Inner>, interval: Duration) {
    let mut snapshot_tick = tokio::time::interval(interval);
    // if saving took longer than snapshot_tick, just skip it over and try in the next round
    snapshot_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let database_id = state.database_id.to_string();
    loop {
      tokio::select! {
        _ = snapshot_tick.tick() => {
          if let Err(err) = state.persister.save_database(&state.workspace_id, &database_id).await {
            tracing::warn!("failed to persist database `{}/{}`: {}", state.workspace_id, state.database_id, err);
          }
        },
        _ = state.shutdown.cancelled() => {
          if let Err(err) = state.persister.save_database(&state.workspace_id, &database_id).await {
            tracing::warn!("failed to persist database on shutdown `{}/{}`: {}", state.workspace_id, state.database_id, err);
          }
          break;
        }
      }
    }
  }
}

struct Inner {
  workspace_id: WorkspaceId,
  /// Database id. For `Database` collab type it's its own id, for `DatabaseRow` it's an ID of
  /// the database, this row belongs to.
  database_id: ObjectId,
  /// State vectors of rows.
  subgroups: DashMap<ObjectId, RwLock<Subgroup>>,
  persister: CollabPersister,
  metrics: Arc<CollabRealtimeMetrics>,
  /// Cancellation token triggered when current collab group is about to be stopped.
  /// This will also shut down all subsequent [Subscription]s.
  shutdown: CancellationToken,
  last_activity: ArcSwap<Instant>,
  collab_update_sink: CollabUpdateSink,
  awareness_update_sink: AwarenessUpdateSink,
}

impl Inner {
  pub fn new(
    workspace_id: WorkspaceId,
    database_id: ObjectId,
    persister: CollabPersister,
    metrics: Arc<CollabRealtimeMetrics>,
  ) -> Self {
    let collab_update_sink = persister
      .collab_redis_stream
      .collab_update_sink(&workspace_id, &database_id.to_string());
    let awareness_update_sink = persister
      .collab_redis_stream
      .awareness_update_sink(&workspace_id, &database_id.to_string());

    Inner {
      workspace_id,
      database_id,
      persister,
      metrics,
      collab_update_sink,
      awareness_update_sink,
      shutdown: CancellationToken::new(),
      last_activity: ArcSwap::new(Instant::now().into()),
      subgroups: DashMap::new(),
    }
  }
}

/// While [DatabaseCollabGroup] takes care of the common tasks shared between `Database` and its
/// `DatabaseRow`s, [Subgroup] contains a state that's specific to a given collab instance.
///
/// It can contain both states for `Database` and its `DatabaseRow`s: it's not limited to only one
/// collab type.
#[derive(Default)]
struct Subgroup {
  state_vector: StateVector,
  seq_no: AtomicU32,
  subscribers: HashMap<RealtimeUser, Subscription>,
}

struct Subscription {
  collab_origin: CollabOrigin,
  sink: Box<dyn SubscriptionSink>,
  shutdown: CancellationToken,
}

impl Subscription {
  fn new<S>(sink: S, collab_origin: CollabOrigin, shutdown: CancellationToken) -> Self
  where
    S: SubscriptionSink + 'static,
  {
    Subscription {
      sink: Box::new(sink),
      collab_origin,
      shutdown,
    }
  }
}

impl Drop for Subscription {
  fn drop(&mut self) {
    tracing::trace!("closing database subscription: {}", self.collab_origin);
    self.shutdown.cancel();
  }
}
