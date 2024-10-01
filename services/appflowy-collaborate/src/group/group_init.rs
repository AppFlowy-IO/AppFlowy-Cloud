use arc_swap::{ArcSwap, ArcSwapOption};
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::lock::RwLock;
use collab::preclude::Collab;
use collab_entity::CollabType;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::{error, event, info, trace};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::Update;

use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::CollabMessage;
use collab_rt_entity::MessageByObjectId;
use collab_stream::client::CollabRedisStream;
use collab_stream::error::StreamError;
use collab_stream::model::{CollabUpdateEvent, StreamBinary};
use collab_stream::stream_group::StreamGroup;
use database::collab::CollabStorage;

use crate::error::RealtimeError;
use crate::group::broadcast::{CollabBroadcast, CollabUpdateStreaming, DataStream, Subscription};
use crate::indexer::Indexer;
use crate::metrics::CollabRealtimeMetrics;

/// A group used to manage a single [Collab] object
pub struct CollabGroup {
  workspace_id: String,
  object_id: String,
  collab_type: CollabType,
  /// A list of subscribers to this group. Each subscriber will receive updates from the
  /// broadcast.
  subscribers: Arc<DashMap<RealtimeUser, Subscription>>,
  persister: Arc<CollabPersister>,
  metrics_calculate: Arc<CollabRealtimeMetrics>,
  /// Cancellation token triggered when current collab group is about to be stopped.
  /// This will also shut down all subsequent [Subscription]s.
  shutdown: CancellationToken,
}

impl Drop for CollabGroup {
  fn drop(&mut self) {
    self.shutdown.cancel();
  }
}

impl CollabGroup {
  #[allow(clippy::too_many_arguments)]
  pub async fn new<S>(
    uid: i64,
    workspace_id: String,
    object_id: String,
    collab_type: CollabType,
    metrics_calculate: Arc<CollabRealtimeMetrics>,
    storage: Arc<S>,
    is_new_collab: bool,
    collab_redis_stream: Arc<CollabRedisStream>,
    persistence_interval: Duration,
    indexer: Option<Arc<dyn Indexer>>,
  ) -> Result<Self, StreamError>
  where
    S: CollabStorage,
  {
    let persister = Arc::new(CollabPersister::new(
      workspace_id.clone(),
      object_id.clone(),
      collab_type.clone(),
      storage,
      collab_redis_stream,
      indexer,
    ));

    let shutdown = CancellationToken::new();
    let subscribers = Arc::new(DashMap::new());

    // setup task used to receive messages from Redis
    {
      let oid = object_id.clone();
      let shutdown = shutdown.clone();
      tokio::spawn(async move {
        if let Err(err) = Self::inbound_task(shutdown).await {
          tracing::warn!("`{}` failed to receive message: {}", oid, err);
        }
      });
    }

    // setup task used to send messages to Redis
    {
      let oid = object_id.clone();
      let shutdown = shutdown.clone();
      tokio::spawn(async move {
        if let Err(err) = Self::outbound_task(shutdown).await {
          tracing::warn!("`{}` failed to send message: {}", oid, err);
        }
      });
    }

    // setup periodic snapshot
    {
      let shutdown = shutdown.clone();
      tokio::spawn(Self::snapshot_task(
        persistence_interval,
        persister.clone(),
        shutdown,
      ));
    }

    Ok(Self {
      workspace_id,
      object_id,
      collab_type,
      subscribers,
      metrics_calculate,
      shutdown,
      persister,
    })
  }

  /// Task used to receive messages from Redis.
  async fn inbound_task(shutdown: CancellationToken) -> Result<(), RealtimeError> {
    todo!()
  }

  /// Task used to send messages to Redis.
  async fn outbound_task(shutdown: CancellationToken) -> Result<(), RealtimeError> {
    todo!()
  }

  async fn snapshot_task(
    interval: Duration,
    persister: Arc<CollabPersister>,
    shutdown: CancellationToken,
  ) {
    let mut snapshot_tick = tokio::time::interval(interval);
    snapshot_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
      tokio::select! {
        _ = snapshot_tick.tick() => {
          if let Err(err) = persister.save().await {
            tracing::warn!("failed to persist document `{}`: {}", persister.object_id, err);
          }
        },
        _ = shutdown.cancelled() => {
          if let Err(err) = persister.save().await {
            tracing::warn!("failed to persist document `{}`: {}", persister.object_id, err);
          }
        }
      }
    }
  }

  pub async fn encode_collab(&self) -> Result<EncodedCollab, RealtimeError> {
    let lock = self.load().await?;
    let encode_collab = lock.encode_collab_v1(|collab| {
      self
        .collab_type
        .validate_require_data(collab)
        .map_err(|err| RealtimeError::Internal(err.into()))
    })?;
    Ok(encode_collab)
  }

  pub fn contains_user(&self, user: &RealtimeUser) -> bool {
    self.subscribers.contains_key(user)
  }

  pub async fn remove_user(&self, user: &RealtimeUser) {
    if let Some((_, mut old_sub)) = self.subscribers.remove(user) {
      trace!("{} remove subscriber from group: {}", self.object_id, user);
      old_sub.stop().await;
    }
  }

  pub fn user_count(&self) -> usize {
    self.subscribers.len()
  }

  /// Subscribes a new connection to the broadcast group for collaborative activities.
  ///
  pub async fn subscribe<Sink, Stream>(
    &self,
    user: &RealtimeUser,
    subscriber_origin: CollabOrigin,
    sink: Sink,
    stream: Stream,
  ) where
    Sink: SinkExt<CollabMessage> + Clone + Send + Sync + Unpin + 'static,
    Stream: StreamExt<Item = MessageByObjectId> + Send + Sync + Unpin + 'static,
    <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error + Send + Sync,
  {
    // create new subscription for new subscriber
    let metrics = self.metrics_calculate.clone();
    let sub = self
      .broadcast
      .subscribe(user, subscriber_origin, sink, stream, metrics);

    if let Some(mut old) = self.subscribers.insert((*user).clone(), sub) {
      tracing::warn!("{}: remove old subscriber: {}", &self.object_id, user);
      old.stop().await;
    }

    if cfg!(debug_assertions) {
      trace!(
        "{}: add new subscriber, current group member: {}",
        &self.object_id,
        self.user_count(),
      );
    }

    trace!(
      "[realtime]:{} new subscriber:{}, connect at:{}, connected members: {}",
      self.object_id,
      user.user_device(),
      user.connect_at,
      self.subscribers.len(),
    );
  }

  /// Check if the group is active. A group is considered active if it has at least one
  /// subscriber
  pub async fn is_inactive(&self) -> bool {
    let modified_at = self.modified_at();

    // In debug mode, we set the timeout to 60 seconds
    if cfg!(debug_assertions) {
      trace!(
        "Group:{}:{} is inactive for {} seconds, subscribers: {}",
        self.object_id,
        self.collab_type,
        modified_at.elapsed().as_secs(),
        self.subscribers.len()
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
            self.object_id,
            self.collab_type,
            modified_at.elapsed().as_secs(),
            self.subscribers.len()
          );
          true
        } else {
          self.subscribers.is_empty()
        }
      } else {
        false
      }
    }
  }

  pub async fn stop(&self) {
    for mut entry in self.subscribers.iter_mut() {
      entry.value_mut().stop().await;
    }
    self.shutdown.cancel();
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
    match self.collab_type {
      CollabType::Document => 30 * 60, // 30 minutes
      CollabType::Database | CollabType::DatabaseRow => 30 * 60, // 30 minutes
      CollabType::WorkspaceDatabase | CollabType::Folder | CollabType::UserAwareness => 6 * 60 * 60, // 6 hours,
      CollabType::Unknown => {
        10 * 60 // 10 minutes
      },
    }
  }
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
      .collab_update_stream(workspace_id, object_id, "collaborate_update_producer")
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

impl CollabUpdateStreaming for CollabUpdateStreamingImpl {
  fn send_update(&self, update: Vec<u8>) -> Result<(), RealtimeError> {
    if let Err(_) = self.sender.send(update) {
      Err(RealtimeError::Internal(anyhow::anyhow!(
        "stream stopped processing incoming updates"
      )))
    } else {
      Ok(())
    }
  }

  fn receive_updates(&self) -> DataStream {
    todo!()
  }

  fn receive_awareness_updates(&self) -> DataStream {
    todo!()
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
    Self {
      workspace_id,
      object_id,
      collab_type,
      storage,
      collab_redis_stream,
      indexer,
      temp_collab: Default::default(),
    }
  }

  /// Drop temp collab i.e. because it was no longer up to date or was not accessed for too long.
  fn reset(&self) {
    self.temp_collab.store(None); // cleanup temp collab
  }

  async fn send_update(&self, update: Vec<u8>) {
    // send updates to redis queue
    todo!()
  }

  async fn send_awareness(&self, awareness_update: Vec<u8>) {
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

  async fn receive_updates(&self) -> DataStream {
    // 1. loop with yield on incoming Redis stream updates
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
