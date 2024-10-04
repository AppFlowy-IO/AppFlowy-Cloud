use anyhow::anyhow;
use arc_swap::{ArcSwap, ArcSwapAny, ArcSwapOption};
use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::lock::RwLock;
use collab::preclude::Collab;
use collab_entity::CollabType;
use dashmap::DashMap;
use futures::{pin_mut, Sink, Stream};
use futures_util::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::fmt::{Display, Formatter};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};
use tokio::time::{interval, MissedTickBehavior};
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::{error, event, info, trace};
use yrs::sync::AwarenessUpdate;
use yrs::updates::decoder::{Decode, DecoderV1, DecoderV2};
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{ReadTxn, StateVector, Update};

use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::{AckCode, BroadcastSync, CollabAck, MessageByObjectId, MsgId};
use collab_rt_entity::{ClientCollabMessage, CollabMessage};
use collab_rt_protocol::{decode_update, Message, MessageReader, RTProtocolError, SyncMessage};
use collab_stream::client::CollabRedisStream;
use collab_stream::collab_update_sink::CollabUpdateSink;
use collab_stream::error::StreamError;
use collab_stream::model::{
  CollabStreamUpdate, CollabUpdateEvent, MessageId, StreamBinary, UpdateFlags,
};
use collab_stream::stream_group::StreamGroup;
use database::collab::CollabStorage;

use crate::error::RealtimeError;
use crate::indexer::Indexer;
use crate::metrics::CollabRealtimeMetrics;
use crate::state::RedisConnectionManager;

/// A group used to manage a single [Collab] object
pub struct CollabGroup {
  state: Arc<CollabGroupState>,
}

impl Drop for CollabGroup {
  fn drop(&mut self) {
    // we're going to use state shutdown to cancel subsequent tasks
    self.state.shutdown.cancel();
  }
}

impl CollabGroup {
  #[allow(clippy::too_many_arguments)]
  pub async fn new<S>(
    uid: i64,
    workspace_id: String,
    object_id: String,
    collab_type: CollabType,
    metrics: Arc<CollabRealtimeMetrics>,
    storage: Arc<S>,
    is_new_collab: bool,
    collab_redis_stream: Arc<CollabRedisStream>,
    persistence_interval: Duration,
    indexer: Option<Arc<dyn Indexer>>,
  ) -> Result<Self, StreamError>
  where
    S: CollabStorage,
  {
    let persister = CollabPersister::new(
      workspace_id.clone(),
      object_id.clone(),
      collab_type.clone(),
      storage,
      collab_redis_stream,
      indexer,
    );

    let state = Arc::new(CollabGroupState {
      workspace_id,
      object_id,
      collab_type,
      subscribers: DashMap::new(),
      metrics,
      shutdown: CancellationToken::new(),
      persister,
      last_activity: ArcSwap::new(Instant::now().into()),
      seq_no: AtomicU32::new(0),
    });

    /*
     NOTE: we don't want to pass `Weak<CollabGroupState>` to tasks and terminate them when they
     cannot be upgraded since we want to be sure that ie. when collab group is to be removed,
     that we're going to call for a final save of the document state.

     For that we use `CancellationToken` instead, which is racing against internal loops of child
     tasks and triggered when this `CollabGroup` is dropped.
    */

    // setup task used to receive messages from Redis
    {
      let state = state.clone();
      tokio::spawn(async move {
        if let Err(err) = Self::inbound_task(state).await {
          tracing::warn!("failed to receive message: {}", err);
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

  #[inline]
  pub fn workspace_id(&self) -> &str {
    &self.state.workspace_id
  }

  #[inline]
  pub fn object_id(&self) -> &str {
    &self.state.object_id
  }

  /// Task used to receive messages from Redis.
  async fn inbound_task(state: Arc<CollabGroupState>) -> Result<(), RealtimeError> {
    let mut updates = state.persister.collab_redis_stream.collab_updates(
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
            Some(Ok(update)) => {
              Self::handle_inbound_update(&state, update).await;
              state.last_activity.store(Arc::new(Instant::now()));
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

  async fn handle_inbound_update(state: &CollabGroupState, update: CollabStreamUpdate) {
    let seq_num = state.seq_no.fetch_add(1, Ordering::SeqCst);
    let message = BroadcastSync::new(update.sender, state.object_id.clone(), update.data, seq_num);
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
    }
  }

  async fn snapshot_task(state: Arc<CollabGroupState>, interval: Duration, is_new_collab: bool) {
    if is_new_collab {
      if let Err(err) = state.persister.save().await {
        tracing::warn!(
          "failed to persist new document `{}`: {}",
          state.object_id,
          err
        );
      }
    }

    let mut snapshot_tick = tokio::time::interval(interval);
    snapshot_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
      tokio::select! {
        _ = snapshot_tick.tick() => {
          if let Err(err) = state.persister.save().await {
            tracing::warn!("failed to persist document `{}`: {}", state.object_id, err);
          }
        },
        _ = state.shutdown.cancelled() => {
          if let Err(err) = state.persister.save().await {
            tracing::warn!("failed to persist document on shutdown `{}`: {}", state.object_id, err);
          }
        }
      }
    }
  }

  pub async fn encode_collab(&self) -> Result<EncodedCollab, RealtimeError> {
    let snapshot = self.state.persister.load().await?;
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

  pub async fn remove_user(&self, user: &RealtimeUser) {
    if let Some(_) = self.state.subscribers.remove(user) {
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
    if let Some(_) = self.state.subscribers.insert((*user).clone(), sub) {
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

  async fn receive_from_client_task<Sink, Stream>(
    state: Arc<CollabGroupState>,
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
            Some(msg) => if let Err(err) =  Self::handle_messages(&state, &mut sink, msg).await {
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

  async fn handle_messages<Sink>(
    state: &CollabGroupState,
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
        match Self::handle_client_message(state, sink, message).await {
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

  /// Handle the message sent from the client
  async fn handle_client_message<Sink>(
    state: &CollabGroupState,
    sink: &mut Sink,
    collab_msg: ClientCollabMessage,
  ) -> Result<CollabAck, RealtimeError>
  where
    Sink: SubscriptionSink + 'static,
  {
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
        state.seq_no.fetch_add(1, Ordering::SeqCst),
      ));
    }

    trace!(
      "Applying client updates: {}, origin:{}",
      collab_msg,
      message_origin
    );

    let payload = collab_msg.payload();
    state.metrics.acquire_collab_lock_count.inc();

    // Spawn a blocking task to handle the message
    let result = Self::handle_message(state, sink, &payload, &message_origin, msg_id).await;

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

  async fn handle_message<Sink>(
    state: &CollabGroupState,
    sink: &mut Sink,
    payload: &[u8],
    message_origin: &CollabOrigin,
    msg_id: MsgId,
  ) -> Result<Option<CollabAck>, RealtimeError>
  where
    Sink: SubscriptionSink + 'static,
  {
    let mut decoder = DecoderV1::from(payload);
    let reader = MessageReader::new(&mut decoder);
    let mut ack_response = None;
    let mut is_sync_step2 = false;
    for msg in reader {
      match msg {
        Ok(msg) => {
          is_sync_step2 = matches!(msg, Message::Sync(SyncMessage::SyncStep2(_)));
          match Self::handle_protocol_message(state, sink, message_origin, msg).await {
            Ok(payload) => {
              state.metrics.apply_update_count.inc();
              // One ClientCollabMessage can have multiple Yrs [Message] in it, but we only need to
              // send one ack back to the client.
              if ack_response.is_none() {
                ack_response = Some(
                  CollabAck::new(
                    message_origin.clone(),
                    state.object_id.to_string(),
                    msg_id,
                    state.seq_no.fetch_add(1, Ordering::SeqCst),
                  )
                  .with_payload(payload.unwrap_or_default()),
                );
              }
            },
            Err(err) => {
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
                  state.seq_no.fetch_add(1, Ordering::SeqCst),
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

  async fn handle_protocol_message<Sink>(
    state: &CollabGroupState,
    sink: &mut Sink,
    origin: &CollabOrigin,
    msg: Message,
  ) -> Result<Option<Vec<u8>>, RTProtocolError>
  where
    Sink: SubscriptionSink + 'static,
  {
    match msg {
      Message::Sync(msg) => match msg {
        SyncMessage::SyncStep1(sv) => Self::handle_sync_step1(state, sink, &sv).await,
        SyncMessage::SyncStep2(update) => Self::handle_sync_step2(state, origin, update).await,
        SyncMessage::Update(update) => Self::handle_update(state, origin, update).await,
      },
      //FIXME: where is the QueryAwareness protocol?
      Message::Awareness(update) => Self::handle_awareness_update(state, origin, update).await,
      Message::Auth(_reason) => Ok(None),
      Message::Custom(_msg) => Ok(None),
    }
  }

  async fn handle_sync_step1<Sink>(
    state: &CollabGroupState,
    sink: &mut Sink,
    remote_sv: &StateVector,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    let snapshot = state
      .persister
      .load()
      .await
      .map_err(|err| RTProtocolError::Internal(err.into()))?;
    let tx = snapshot.collab.transact();
    let doc_state = tx.encode_state_as_update_v1(remote_sv);
    let local_sv = tx.state_vector();
    drop(tx);
    // Retrieve the latest document state from the client after they return online from offline editing.
    let mut encoder = EncoderV1::new();
    Message::Sync(SyncMessage::SyncStep2(doc_state)).encode(&mut encoder);

    //FIXME: this should never happen as response to sync step 1 from the client, but rather be
    //  send when a connection is established
    Message::Sync(SyncMessage::SyncStep1(local_sv)).encode(&mut encoder);
    Ok(Some(encoder.to_vec()))
  }

  async fn handle_sync_step2(
    state: &CollabGroupState,
    origin: &CollabOrigin,
    update: Vec<u8>,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    state.metrics.apply_update_size.observe(update.len() as f64);
    let start = tokio::time::Instant::now();
    state.persister.send_update(origin.clone(), update).await;
    let elapsed = start.elapsed();
    state
      .metrics
      .apply_update_time
      .observe(elapsed.as_millis() as f64);
    Ok(None)
  }

  async fn handle_update(
    state: &CollabGroupState,
    origin: &CollabOrigin,
    update: Vec<u8>,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    Self::handle_sync_step2(state, origin, update).await
  }

  async fn handle_awareness_update(
    state: &CollabGroupState,
    origin: &CollabOrigin,
    update: AwarenessUpdate,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    state.persister.send_awareness(origin, update).await;

    //let mut lock = collab.write().await;
    //let collab = (*lock).borrow_mut();
    //collab.get_awareness().apply_update(update)?;
    todo!()
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
  pub async fn is_inactive(&self) -> bool {
    let modified_at = self.modified_at();

    // In debug mode, we set the timeout to 60 seconds
    if cfg!(debug_assertions) {
      trace!(
        "Group:{}:{} is inactive for {} seconds, subscribers: {}",
        self.state.object_id,
        self.state.collab_type,
        modified_at.elapsed().as_secs(),
        self.state.subscribers.len()
      );
      modified_at.elapsed().as_secs() > 60 * 3
    } else {
      let elapsed_secs = modified_at.elapsed().as_secs();
      if elapsed_secs > self.timeout_secs() {
        // Mark the group as inactive if it has been inactive for more than 3 hours, regardless of the number of subscribers.
        // Otherwise, return `true` only if there are no subscribers remaining in the group.
        // If a client modifies a group that has already been marked as inactive (removed),
        // the client will automatically send an initialization sync to reinitialize the group.
        const MAXIMUM_SECS: u64 = 3 * 60 * 60;
        if elapsed_secs > MAXIMUM_SECS {
          info!(
            "Group:{}:{} is inactive for {} seconds, subscribers: {}",
            self.state.object_id,
            self.state.collab_type,
            modified_at.elapsed().as_secs(),
            self.state.subscribers.len()
          );
          true
        } else {
          self.state.subscribers.is_empty()
        }
      } else {
        false
      }
    }
  }

  /// Returns the timeout duration in seconds for different collaboration types.
  ///
  /// Collaborative entities vary in their activity and interaction patterns, necessitating
  /// different timeout durations to balance efficient resource management with a positive
  /// user experience. This function assigns a timeout duration to each collaboration type,
  /// ensuring that resources are utilized judiciously without compromising user engagement.
  ///
  /// # Returns
  /// A `u64` representing the timeout duration in seconds for the collaboration type in question.
  #[inline]
  fn timeout_secs(&self) -> u64 {
    match self.state.collab_type {
      CollabType::Document => 30 * 60, // 30 minutes
      CollabType::Database | CollabType::DatabaseRow => 30 * 60, // 30 minutes
      CollabType::WorkspaceDatabase | CollabType::Folder | CollabType::UserAwareness => 6 * 60 * 60, // 6 hours,
      CollabType::Unknown => {
        10 * 60 // 10 minutes
      },
    }
  }
}

/// Inner state of [CollabGroup] that's private and hidden behind Arc, so that it can be moved into
/// tasks.
struct CollabGroupState {
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
}

struct CollabUpdateStreamingImpl {
  sender: mpsc::UnboundedSender<Vec<u8>>,
}

impl CollabUpdateStreamingImpl {
  async fn new(
    workspace_id: &str,
    object_id: &str,
    collab_redis_stream: &CollabRedisStream,
  ) -> Result<Self, StreamError> {
    let stream = collab_redis_stream
      .collab_update_stream_group(workspace_id, object_id, "collaborate_update_producer")
      .await?;
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
      if let Err(err) = Self::consume_messages(receiver, stream).await {
        error!("Failed to consume incoming updates: {}", err);
      }
    });
    Ok(Self { sender })
  }

  async fn consume_messages(
    mut receiver: mpsc::UnboundedReceiver<Vec<u8>>,
    mut stream: StreamGroup,
  ) -> Result<(), RealtimeError> {
    while let Some(update) = receiver.recv().await {
      let mut update_count = 1;
      let update = {
        let mut updates = VecDeque::new();
        // there may be already more messages inside waiting, try to read them all right away
        while let Ok(update) = receiver.try_recv() {
          updates.push_back(Update::decode_v1(&update)?);
        }
        if updates.is_empty() {
          update // no following messages
        } else {
          update_count += updates.len();
          // prepend first update and merge them all together
          updates.push_front(Update::decode_v1(&update)?);
          Update::merge_updates(updates).encode_v1()
        }
      };

      let msg = StreamBinary::try_from(CollabUpdateEvent::UpdateV1 {
        encode_update: update,
      })?;
      stream.insert_messages(vec![msg]).await?;
      trace!("Sent cumulative ({}) collab update to redis", update_count);
    }
    Ok(())
  }
}

pub trait SubscriptionSink:
  Sink<CollabMessage, Error = RealtimeError> + Send + Sync + Unpin
{
}
impl<T> SubscriptionSink for T where
  T: Sink<CollabMessage, Error = RealtimeError> + Send + Sync + Unpin
{
}

pub trait SubscriptionStream: Stream<Item = MessageByObjectId> + Send + Sync + Unpin {}
impl<T> SubscriptionStream for T where T: Stream<Item = MessageByObjectId> + Send + Sync + Unpin {}

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
    self.shutdown.cancel();
  }
}

struct CollabPersister {
  workspace_id: String,
  object_id: String,
  collab_type: CollabType,
  storage: Arc<dyn CollabStorage>,
  collab_redis_stream: Arc<CollabRedisStream>,
  indexer: Option<Arc<dyn Indexer>>,
  /// Collab stored temporarily.
  temp_collab: ArcSwapOption<CollabSnapshot>,
  update_sink: CollabUpdateSink,
}

impl CollabPersister {
  pub fn new(
    workspace_id: String,
    object_id: String,
    collab_type: CollabType,
    storage: Arc<dyn CollabStorage>,
    collab_redis_stream: Arc<CollabRedisStream>,
    indexer: Option<Arc<dyn Indexer>>,
  ) -> Self {
    let update_sink = collab_redis_stream.collab_update_sink(&workspace_id, &object_id);
    Self {
      workspace_id,
      object_id,
      collab_type,
      storage,
      collab_redis_stream,
      indexer,
      update_sink,
      temp_collab: Default::default(),
    }
  }

  /// Drop temp collab i.e. because it was no longer up to date or was not accessed for too long.
  fn reset(&self) {
    self.temp_collab.store(None); // cleanup temp collab
  }

  async fn send_update(
    &self,
    sender: CollabOrigin,
    update: Vec<u8>,
  ) -> Result<MessageId, StreamError> {
    // send updates to redis queue
    let msg_id = self
      .update_sink
      .send(&CollabStreamUpdate::new(
        update,
        sender,
        UpdateFlags::default(),
      ))
      .await?;
    Ok(msg_id)
  }

  async fn send_awareness(&self, sender_session: &CollabOrigin, awareness_update: AwarenessUpdate) {
    // send awareness updates to redis queue: is it needed? What are we using awareness for here?
    todo!()
  }

  async fn load(&self) -> Result<Arc<CollabSnapshot>, RealtimeError> {
    match self.temp_collab.load_full() {
      Some(collab) => Ok(collab), // return cached collab
      None => self.force_load().await,
    }
  }

  async fn force_load(&self) -> Result<Arc<CollabSnapshot>, RealtimeError> {
    // 1. Try to load the latest snapshot from storage
    // 2. consume all Redis updates on top of it (keep redis msg id)
    todo!()
  }

  async fn save(&self) -> Result<(), RealtimeError> {
    // 1. try to acquire lock
    // 2. if successful -> self.load()
    // 3.     if collab has any changes (any redis updates were applied):
    // 4.         generate embeddings
    // 5.         store collab
    // 6.         prune any redis msg ids older than 5 min. since collab snapshot time
    todo!()
  }
}

pub struct CollabSnapshot {
  pub collab: Collab,
  pub last_msg_id: String,
}
