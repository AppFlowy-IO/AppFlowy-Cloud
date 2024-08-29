use std::collections::VecDeque;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::lock::RwLock;
use collab::preclude::Collab;
use collab_entity::CollabType;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
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
use crate::group::broadcast::{CollabBroadcast, CollabUpdateStreaming, Subscription};
use crate::group::persistence::GroupPersistence;
use crate::indexer::Indexer;
use crate::metrics::CollabMetricsCalculate;

/// A group used to manage a single [Collab] object
pub struct CollabGroup {
  pub workspace_id: String,
  pub object_id: String,
  collab: Arc<RwLock<Collab>>,
  collab_type: CollabType,
  /// A broadcast used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
  /// to subscribes.
  broadcast: CollabBroadcast,
  /// A list of subscribers to this group. Each subscriber will receive updates from the
  /// broadcast.
  subscribers: DashMap<RealtimeUser, Subscription>,
  metrics_calculate: CollabMetricsCalculate,
  destroy_group_tx: mpsc::Sender<Arc<RwLock<Collab>>>,
}

impl Drop for CollabGroup {
  fn drop(&mut self) {
    trace!("Drop collab group:{}", self.object_id);
  }
}

impl CollabGroup {
  #[allow(clippy::too_many_arguments)]
  pub async fn new<S>(
    uid: i64,
    workspace_id: String,
    object_id: String,
    collab_type: CollabType,
    collab: Arc<RwLock<Collab>>,
    metrics_calculate: CollabMetricsCalculate,
    storage: Arc<S>,
    is_new_collab: bool,
    collab_redis_stream: Arc<CollabRedisStream>,
    persistence_interval: Duration,
    edit_state_max_count: u32,
    edit_state_max_secs: i64,
    indexer: Option<Arc<dyn Indexer>>,
  ) -> Result<Self, StreamError>
  where
    S: CollabStorage,
  {
    let edit_state = Arc::new(EditState::new(
      edit_state_max_count,
      edit_state_max_secs,
      is_new_collab,
    ));
    let broadcast = {
      let lock = collab.read().await;
      CollabBroadcast::new(
        &object_id,
        10,
        edit_state.clone(),
        &lock,
        CollabUpdateStreamingImpl::new(&workspace_id, &object_id, &collab_redis_stream).await?,
      )
    };
    let (destroy_group_tx, rx) = mpsc::channel(1);

    tokio::spawn(
      GroupPersistence::new(
        workspace_id.clone(),
        object_id.clone(),
        uid,
        storage,
        edit_state.clone(),
        Arc::downgrade(&collab),
        collab_type.clone(),
        persistence_interval,
        indexer,
      )
      .run(rx),
    );

    Ok(Self {
      workspace_id,
      object_id,
      collab_type,
      collab,
      broadcast,
      subscribers: Default::default(),
      metrics_calculate,
      destroy_group_tx,
    })
  }

  pub async fn encode_collab(&self) -> Result<EncodedCollab, RealtimeError> {
    let lock = self.collab.read().await;
    let encode_collab =
      lock.encode_collab_v1(|collab| self.collab_type.validate_require_data(collab))?;
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
    let sub = self.broadcast.subscribe(
      user,
      subscriber_origin,
      sink,
      stream,
      Arc::downgrade(&self.collab),
      self.metrics_calculate.clone(),
    );

    if let Some(mut old) = self.subscribers.insert((*user).clone(), sub) {
      old.stop().await;
    }

    if cfg!(debug_assertions) {
      event!(
        tracing::Level::TRACE,
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
    let modified_at = self.broadcast.modified_at.lock();

    // In debug mode, we set the timeout to 60 seconds
    if cfg!(debug_assertions) {
      trace!(
        "Group:{} is inactive for {} seconds, subscribers: {}",
        self.object_id,
        modified_at.elapsed().as_secs(),
        self.subscribers.len()
      );
      modified_at.elapsed().as_secs() > 60 * 60
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
            "Group:{} is inactive for {} seconds, subscribers: {}",
            self.object_id,
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
    let _ = self.destroy_group_tx.send(self.collab.clone()).await;
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
      CollabType::Document => 10 * 60, // 10 minutes
      CollabType::Database | CollabType::DatabaseRow => 30 * 60, // 30 minutes
      CollabType::WorkspaceDatabase | CollabType::Folder | CollabType::UserAwareness => 6 * 60 * 60, // 6 hours,
      CollabType::Unknown => {
        10 * 60 // 10 minutes
      },
    }
  }
}

pub(crate) struct EditState {
  /// Clients rely on `edit_count` to verify message ordering. A non-continuous sequence suggests
  /// missing updates, prompting the client to request an initial synchronization.
  /// Continuous sequence numbers ensure the client receives and displays updates in the correct order.
  ///
  edit_counter: AtomicU32,
  prev_edit_count: AtomicU32,
  prev_flush_timestamp: AtomicI64,

  max_edit_count: u32,
  max_secs: i64,
  /// Indicate the collab object is just created in the client and not exist in server database.
  is_new: AtomicBool,
  /// Indicate the collab is ready to save to disk.
  /// If is_ready_to_save is true, which means the collab contains the requirement data and ready to save to disk.
  is_ready_to_save: AtomicBool,
}

impl Display for EditState {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
        f,
        "EditState {{ edit_counter: {}, prev_edit_count: {},  max_edit_count: {}, max_secs: {}, is_new: {}, is_ready_to_save: {}",
        self.edit_counter.load(Ordering::SeqCst),
        self.prev_edit_count.load(Ordering::SeqCst),
        self.max_edit_count,
        self.max_secs,
        self.is_new.load(Ordering::SeqCst),
        self.is_ready_to_save.load(Ordering::SeqCst),
        )
  }
}

impl EditState {
  fn new(max_edit_count: u32, max_secs: i64, is_new: bool) -> Self {
    Self {
      edit_counter: AtomicU32::new(0),
      prev_edit_count: Default::default(),
      prev_flush_timestamp: AtomicI64::new(chrono::Utc::now().timestamp()),
      max_edit_count,
      max_secs,
      is_new: AtomicBool::new(is_new),
      is_ready_to_save: AtomicBool::new(false),
    }
  }

  pub(crate) fn edit_count(&self) -> u32 {
    self.edit_counter.load(Ordering::SeqCst)
  }

  /// Increments the edit count and returns the old value
  pub(crate) fn increment_edit_count(&self) -> u32 {
    self
      .edit_counter
      .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
        Some(current + 1)
      })
      // safety: unwrap when returning the new value
      .unwrap()
  }

  pub(crate) fn tick(&self) {
    self
      .prev_edit_count
      .store(self.edit_counter.load(Ordering::SeqCst), Ordering::SeqCst);
    self
      .prev_flush_timestamp
      .store(chrono::Utc::now().timestamp(), Ordering::SeqCst);
  }

  pub(crate) fn is_edit(&self) -> bool {
    self.edit_counter.load(Ordering::SeqCst) != self.prev_edit_count.load(Ordering::SeqCst)
  }

  pub(crate) fn is_new(&self) -> bool {
    self.is_new.load(Ordering::SeqCst)
  }

  pub(crate) fn set_is_new(&self, is_new: bool) {
    self.is_new.store(is_new, Ordering::SeqCst);
  }

  pub(crate) fn set_ready_to_save(&self) {
    self.is_ready_to_save.store(true, Ordering::Relaxed);
  }

  pub(crate) fn should_save_to_disk(&self) -> bool {
    if !self.is_ready_to_save.load(Ordering::Relaxed) {
      return false;
    }

    if self.is_new.load(Ordering::Relaxed) {
      return true;
    }

    let current_edit_count = self.edit_counter.load(Ordering::SeqCst);
    let prev_edit_count = self.prev_edit_count.load(Ordering::SeqCst);

    // If the collab is new, save it to disk and reset the flag
    if self.is_new.load(Ordering::SeqCst) {
      return true;
    }

    if current_edit_count == prev_edit_count {
      return false;
    }

    // Check if the edit count exceeds the maximum allowed since the last save
    let edit_count_exceeded = (current_edit_count > prev_edit_count)
      && ((current_edit_count - prev_edit_count) >= self.max_edit_count);

    // Calculate the time since the last flush and check if it exceeds the maximum allowed
    let now = chrono::Utc::now().timestamp();
    let prev_flush_timestamp = self.prev_flush_timestamp.load(Ordering::SeqCst);
    let time_exceeded =
      (now > prev_flush_timestamp) && (now - prev_flush_timestamp >= self.max_secs);

    // Determine if we should save based on either condition being met
    edit_count_exceeded || (current_edit_count != prev_edit_count && time_exceeded)
  }
}

struct CollabUpdateStreamingImpl {
  sender: mpsc::UnboundedSender<Vec<u8>>,
  stopped: Arc<AtomicBool>,
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
    let stopped = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::unbounded_channel();
    let cloned_stopped = stopped.clone();
    tokio::spawn(async move {
      if let Err(err) = Self::consume_messages(receiver, stream).await {
        error!("Failed to consume incoming updates: {}", err);
      }
      cloned_stopped.store(true, Ordering::SeqCst);
    });
    Ok(Self { sender, stopped })
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

  pub fn is_stopped(&self) -> bool {
    self.stopped.load(Ordering::SeqCst)
  }
}

impl CollabUpdateStreaming for CollabUpdateStreamingImpl {
  fn send_update(&self, update: Vec<u8>) -> Result<(), RealtimeError> {
    if self.is_stopped() {
      Err(RealtimeError::Internal(anyhow::anyhow!(
        "stream stopped processing incoming updates"
      )))
    } else if let Err(err) = self.sender.send(update) {
      Err(RealtimeError::Internal(err.into()))
    } else {
      Ok(())
    }
  }
}

#[cfg(test)]
mod tests {
  use crate::group::group_init::EditState;

  #[test]
  fn edit_state_test() {
    let edit_state = EditState::new(10, 10, false);
    edit_state.set_ready_to_save();
    edit_state.increment_edit_count();

    for _ in 0..10 {
      edit_state.increment_edit_count();
    }
    assert!(edit_state.should_save_to_disk());
    assert!(edit_state.should_save_to_disk());
    edit_state.tick();
    assert!(!edit_state.should_save_to_disk());
  }
}
