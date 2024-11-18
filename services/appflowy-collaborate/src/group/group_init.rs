use crate::error::RealtimeError;
use crate::group::collab_group::{SubscriptionSink, SubscriptionStream};
use crate::group::persister::CollabPersister;
use crate::indexer::Indexer;
use crate::metrics::CollabRealtimeMetrics;
use anyhow::anyhow;
use app_error::AppError;
use arc_swap::ArcSwap;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::lock::RwLock;
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::{
  AckCode, AwarenessSync, BroadcastSync, CollabAck, MessageByObjectId, MsgId,
};
use collab_rt_entity::{ClientCollabMessage, CollabMessage};
use collab_rt_protocol::{Message, MessageReader, RTProtocolError, SyncMessage};
use collab_stream::client::CollabRedisStream;
use collab_stream::collab_update_sink::{AwarenessUpdateSink, CollabUpdateSink};
use collab_stream::error::StreamError;
use collab_stream::model::{AwarenessStreamUpdate, CollabStreamUpdate, MessageId, UpdateFlags};
use dashmap::DashMap;
use database::collab::{CollabStorage, GetCollabOrigin};
use database_entity::dto::{
  AFCollabEmbeddings, CollabParams, InsertSnapshotParams, QueryCollabParams,
};
use futures::{pin_mut, Sink, Stream};
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, trace};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{ReadTxn, StateVector, Update};

/// A group used to manage a single [Collab] object
#[derive(Clone)]
pub struct DefaultCollabGroup {
  state: Arc<Inner>,
}

/// Inner state of [CollabGroup] that's private and hidden behind Arc, so that it can be moved into
/// tasks.
struct Inner {
  workspace_id: String,
  object_id: String,
  collab_type: CollabType,
  /// A list of subscribers to this group. Each subscriber will receive updates from the
  /// broadcast.
  subscribers: DashMap<RealtimeUser, Subscription>,
  persister: CollabPersister,
  metrics: Arc<CollabRealtimeMetrics>,
  /// Cancellation token triggered when current collab group is about to be stopped.
  /// This will also shut down all subsequent [Subscription]s.
  shutdown: CancellationToken,
  last_activity: ArcSwap<Instant>,
  seq_no: AtomicU32,
  /// The most recent state vector from a redis update.
  state_vector: RwLock<StateVector>,
  collab_update_sink: CollabUpdateSink,
  awareness_update_sink: AwarenessUpdateSink,
}

impl Drop for DefaultCollabGroup {
  fn drop(&mut self) {
    // we're going to use state shutdown to cancel subsequent tasks
    self.state.shutdown.cancel();
  }
}

impl DefaultCollabGroup {
  #[allow(clippy::too_many_arguments)]
  pub async fn new(
    workspace_id: String,
    object_id: String,
    collab_type: CollabType,
    metrics: Arc<CollabRealtimeMetrics>,
    persister: CollabPersister,
    is_new_collab: bool,
    persistence_interval: Duration,
  ) -> Result<Self, StreamError> {
    let collab_update_sink = persister
      .collab_redis_stream
      .collab_update_sink(&workspace_id, &object_id);
    let awareness_update_sink = persister
      .collab_redis_stream
      .awareness_update_sink(&workspace_id, &object_id);

    let state = Arc::new(Inner {
      workspace_id,
      object_id,
      collab_type,
      collab_update_sink,
      awareness_update_sink,
      subscribers: DashMap::new(),
      metrics,
      shutdown: CancellationToken::new(),
      persister,
      last_activity: ArcSwap::new(Instant::now().into()),
      seq_no: AtomicU32::new(0),
      state_vector: Default::default(),
    });

    /*
     NOTE: we don't want to pass `Weak<CollabGroupState>` to tasks and terminate them when they
     cannot be upgraded since we want to be sure that ie. when collab group is to be removed,
     that we're going to call for a final save of the document state.

     For that we use `CancellationToken` instead, which is racing against internal loops of child
     tasks and triggered when this `CollabGroup` is dropped.
    */

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
      tokio::spawn(Self::snapshot_task(
        state.clone(),
        persistence_interval,
        is_new_collab,
      ));
    }

    Ok(Self { state })
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

  /// Handle a single collab update received from Redis stream.
  async fn handle_inbound_update(state: &Inner, update: CollabStreamUpdate) {
    // update state vector based on incoming message
    let mut sv = state.state_vector.write().await;
    sv.merge(update.state_vector);
    drop(sv);

    let seq_num = state.seq_no.fetch_add(1, Ordering::SeqCst) + 1;
    tracing::trace!(
      "broadcasting collab update from {} ({} bytes) - seq_num: {}",
      update.sender,
      update.data.len(),
      seq_num
    );
    let payload = Message::Sync(SyncMessage::Update(update.data)).encode_v1();
    let message = BroadcastSync::new(update.sender, state.object_id.clone(), payload, seq_num);
    for mut e in state.subscribers.iter_mut() {
      let subscription = e.value_mut();
      if message.origin == subscription.collab_origin {
        continue; // don't send update to its sender
      }

      if let Err(err) = subscription.sink.send(message.clone().into()).await {
        tracing::debug!(
          "failed to send collab `{}` update to `{}`: {}",
          state.object_id,
          subscription.collab_origin,
          err
        );
      }

      state.last_activity.store(Arc::new(Instant::now()));
    }
  }

  /// Task used to receive awareness updates from Redis.
  async fn inbound_awareness_task(state: Arc<Inner>) -> Result<(), RealtimeError> {
    let updates = state.persister.collab_redis_stream.awareness_updates(
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
            Some(Ok(awareness_update)) => {
              Self::handle_inbound_awareness(&state, awareness_update).await;
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

  /// Handle a single awareness update comming from Redis stream.
  async fn handle_inbound_awareness(state: &Inner, update: AwarenessStreamUpdate) {
    tracing::trace!(
      "broadcasting awareness update from {} ({} bytes)",
      update.sender,
      update.data.len()
    );
    let sender = update.sender;
    let message = AwarenessSync::new(
      state.object_id.clone(),
      Message::Awareness(update.data).encode_v1(),
      CollabOrigin::Empty,
    );
    for mut e in state.subscribers.iter_mut() {
      let subscription = e.value_mut();
      if sender == subscription.collab_origin {
        continue; // don't send update to its sender
      }

      if let Err(err) = subscription.sink.send(message.clone().into()).await {
        tracing::debug!(
          "failed to send awareness `{}` update to `{}`: {}",
          state.object_id,
          subscription.collab_origin,
          err
        );
      }

      state.last_activity.store(Arc::new(Instant::now()));
    }
  }

  /// Async task which periodically will try to pull collab updates from Redis and - if there
  /// are any - save them as snapshots and collab doc state.
  async fn snapshot_task(state: Arc<Inner>, interval: Duration, is_new_collab: bool) {
    if is_new_collab {
      tracing::trace!("persisting new collab for {}", state.object_id);
      if let Err(err) = state
        .persister
        .save(
          &state.workspace_id,
          &state.object_id,
          state.collab_type.clone(),
        )
        .await
      {
        tracing::warn!(
          "failed to persist new document `{}`: {}",
          state.object_id,
          err
        );
      }
    }

    let mut snapshot_tick = tokio::time::interval(interval);
    // if saving took longer than snapshot_tick, just skip it over and try in the next round
    snapshot_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
      tokio::select! {
        _ = snapshot_tick.tick() => {
          if let Err(err) = state.persister.save(
              &state.workspace_id,
              &state.object_id,
              state.collab_type.clone()).await {
            tracing::warn!("failed to persist collab `{}/{}`: {}", state.workspace_id, state.object_id, err);
          }
        },
        _ = state.shutdown.cancelled() => {
          if let Err(err) = state.persister.save(
              &state.workspace_id,
              &state.object_id,
              state.collab_type.clone()).await {
            tracing::warn!("failed to persist collab on shutdown `{}/{}`: {}", state.workspace_id, state.object_id, err);
          }
          break;
        }
      }
    }
  }

  pub async fn encode_collab(&self) -> Result<EncodedCollab, RealtimeError> {
    let snapshot = self
      .state
      .persister
      .load_compact(
        &self.state.workspace_id,
        &self.state.object_id,
        self.state.collab_type.clone(),
      )
      .await?;
    let encode_collab = snapshot.collab.encode_collab_v1(|collab| {
      self
        .state
        .collab_type
        .validate_require_data(collab)
        .map_err(|err| RealtimeError::Internal(err.into()))
    })?;
    Ok(encode_collab)
  }

  pub fn contains_user(&self, user: &RealtimeUser) -> bool {
    self.state.subscribers.contains_key(user)
  }

  pub fn remove_user(&self, user: &RealtimeUser) {
    if self.state.subscribers.remove(user).is_some() {
      trace!(
        "{} remove subscriber from group: {}",
        self.state.object_id,
        user
      );
    }
  }

  pub fn user_count(&self) -> usize {
    self.state.subscribers.len()
  }

  pub fn modified_at(&self) -> Instant {
    *self.state.last_activity.load_full()
  }

  /// Subscribes a new connection to the broadcast group for collaborative activities.
  ///
  pub fn subscribe<Sink, Stream>(
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
      self.state.clone(),
      sink.clone(),
      stream,
      subscriber_origin.clone(),
    ));

    let sub = Subscription::new(sink, subscriber_origin, subscriber_shutdown);
    if self
      .state
      .subscribers
      .insert((*user).clone(), sub)
      .is_some()
    {
      tracing::warn!("{}: remove old subscriber: {}", &self.state.object_id, user);
    }

    if cfg!(debug_assertions) {
      trace!(
        "{}: add new subscriber, current group member: {}",
        &self.state.object_id,
        self.user_count(),
      );
    }

    trace!(
      "[realtime]:{} new subscriber:{}, connect at:{}, connected members: {}",
      self.state.object_id,
      user.user_device(),
      user.connect_at,
      self.state.subscribers.len(),
    );
  }

  /// Async task that will pull messages from `client_stream` and handle them, optionally sending
  /// response messages to `client_sink`.
  async fn receive_from_client_task<Sink, Stream>(
    state: Arc<Inner>,
    mut client_sink: Sink,
    mut client_stream: Stream,
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
        msg = client_stream.next() => {
          match msg {
            None => break,
            Some(msg) => if let Err(err) =  Self::handle_messages(&state, &mut client_sink, msg).await {
              tracing::warn!(
                "collab `{}` failed to handle message from `{}`: {}",
                state.object_id,
                origin,
                err
              );

            }
          }
        }
      }
    }
  }

  /// Handles group of messages coming from the client.
  async fn handle_messages<Sink>(
    state: &Inner,
    sink: &mut Sink,
    msg: MessageByObjectId,
  ) -> Result<(), RealtimeError>
  where
    Sink: SubscriptionSink + 'static,
  {
    for (message_object_id, messages) in msg {
      if state.object_id != message_object_id {
        error!(
          "Expect object id:{} but got:{}",
          state.object_id, message_object_id
        );
        continue;
      }
      for message in messages {
        match Self::handle_client_message(state, message).await {
          Ok(response) => {
            trace!("[realtime]: sending response: {}", response);
            match sink.send(response.into()).await {
              Ok(()) => {},
              Err(err) => {
                trace!("[realtime]: send failed: {}", err);
                break;
              },
            }
          },
          Err(err) => {
            error!(
              "Error handling collab message for object_id: {}: {}",
              message_object_id, err
            );
            break;
          },
        }
      }
    }
    Ok(())
  }

  /// Handle a single message sent from the client.
  async fn handle_client_message(
    state: &Inner,
    collab_msg: ClientCollabMessage,
  ) -> Result<CollabAck, RealtimeError> {
    let msg_id = collab_msg.msg_id();
    let message_origin = collab_msg.origin().clone();

    // If the payload is empty, we don't need to apply any updates .
    // Currently, only the ping message should has an empty payload.
    if collab_msg.payload().is_empty() {
      if !matches!(collab_msg, ClientCollabMessage::ClientCollabStateCheck(_)) {
        error!("receive unexpected empty payload message:{}", collab_msg);
      }
      return Ok(CollabAck::new(
        message_origin,
        state.object_id.to_string(),
        msg_id,
        state.seq_no.load(Ordering::SeqCst),
      ));
    }

    trace!(
      "Applying client updates: {}, origin:{}",
      collab_msg,
      message_origin
    );

    let payload = collab_msg.payload();

    // Spawn a blocking task to handle the message
    let result = Self::handle_message(state, payload, &message_origin, msg_id).await;

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
          match Self::handle_protocol_message(state, message_origin, msg).await {
            Ok(payload) => {
              // One ClientCollabMessage can have multiple Yrs [Message] in it, but we only need to
              // send one ack back to the client.
              if ack_response.is_none() {
                ack_response = Some(
                  CollabAck::new(
                    message_origin.clone(),
                    state.object_id.to_string(),
                    msg_id,
                    state.seq_no.load(Ordering::SeqCst),
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
                  state.object_id.to_string(),
                  msg_id,
                  state.seq_no.load(Ordering::SeqCst),
                )
                .with_code(code)
                .with_payload(payload),
              );

              break;
            },
          }
        },
        Err(e) => {
          error!("{} => parse sync message failed: {:?}", state.object_id, e);
          break;
        },
      }
    }
    Ok(ack_response)
  }

  async fn handle_protocol_message(
    state: &Inner,
    origin: &CollabOrigin,
    msg: Message,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    match msg {
      Message::Sync(msg) => match msg {
        SyncMessage::SyncStep1(sv) => Self::handle_sync_step1(state, &sv).await,
        SyncMessage::SyncStep2(update) => Self::handle_sync_step2(state, origin, update).await,
        SyncMessage::Update(update) => Self::handle_update(state, origin, update).await,
      },
      //FIXME: where is the QueryAwareness protocol?
      Message::Awareness(update) => Self::handle_awareness_update(state, origin, update).await,
      Message::Auth(_reason) => Ok(None),
      Message::Custom(_msg) => Ok(None),
    }
  }

  async fn handle_sync_step1(
    state: &Inner,
    remote_sv: &StateVector,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    if let Ok(sv) = state.state_vector.try_read() {
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
      let state_vector = state.state_vector.read().await;
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
        None,
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
    state: &Inner,
    origin: &CollabOrigin,
    update: Vec<u8>,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    Self::handle_sync_step2(state, origin, update).await
  }

  async fn handle_awareness_update(
    state: &Inner,
    origin: &CollabOrigin,
    data: Vec<u8>,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    let msg = AwarenessStreamUpdate {
      data,
      sender: origin.clone(),
      row: None,
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

  /// Check if the group is active. A group is considered active if it has at least one
  /// subscriber
  pub fn is_inactive(&self) -> bool {
    self.state.shutdown.is_cancelled() || self.state.subscribers.is_empty()
  }
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
    tracing::trace!("closing subscription: {}", self.collab_origin);
    self.shutdown.cancel();
  }
}
