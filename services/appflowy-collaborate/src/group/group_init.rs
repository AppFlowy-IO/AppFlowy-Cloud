use std::fmt::Display;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::lock::RwLock;
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::CollabMessage;
use collab_rt_entity::MessageByObjectId;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use tokio_util::sync::CancellationToken;
use tracing::{event, info, trace};
use yrs::{ReadTxn, StateVector};

use collab_stream::error::StreamError;

use database::collab::CollabStorage;

use crate::error::RealtimeError;
use crate::group::broadcast::{CollabBroadcast, Subscription};
use crate::group::persistence::GroupPersistence;
use crate::indexer::Indexer;
use crate::metrics::CollabRealtimeMetrics;

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
  metrics_calculate: Arc<CollabRealtimeMetrics>,
  cancel: CancellationToken,
}

impl CollabGroup {
  #[allow(clippy::too_many_arguments)]
  pub fn new<S>(
    uid: i64,
    workspace_id: String,
    object_id: String,
    collab_type: CollabType,
    collab: Collab,
    metrics_calculate: Arc<CollabRealtimeMetrics>,
    storage: Arc<S>,
    is_new_collab: bool,
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
    let broadcast = CollabBroadcast::new(&object_id, 1000, edit_state.clone(), &collab);
    let cancel = CancellationToken::new();

    let collab = Arc::new(RwLock::new(collab));
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
        cancel.clone(),
      )
      .run(),
    );

    Ok(Self {
      workspace_id,
      object_id,
      collab_type,
      collab,
      broadcast,
      subscribers: Default::default(),
      metrics_calculate,
      cancel,
    })
  }

  pub async fn calculate_missing_update(
    &self,
    state_vector: StateVector,
  ) -> Result<Vec<u8>, RealtimeError> {
    let guard = self.collab.read().await;
    let txn = guard.transact();
    let update = txn.encode_state_as_update_v1(&state_vector);
    drop(txn);
    drop(guard);

    Ok(update)
  }

  pub async fn encode_collab(&self) -> Result<EncodedCollab, RealtimeError> {
    let lock = self.collab.read().await;
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

  pub fn remove_user(&self, user: &RealtimeUser) {
    if self.subscribers.remove(user).is_some() {
      trace!("{} remove subscriber from group: {}", self.object_id, user);
    }
  }

  pub fn user_count(&self) -> usize {
    self.subscribers.len()
  }

  /// Subscribes a new connection to the broadcast group for collaborative activities.
  pub fn subscribe<Sink, Stream>(
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
      self.cancel.child_token(),
    );

    if let Some(old) = self.subscribers.insert((*user).clone(), sub) {
      tracing::warn!("{}: remove old subscriber: {}", &self.object_id, user);
      drop(old);
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
  pub fn is_inactive(&self) -> bool {
    let modified_at = *self.broadcast.modified_at.lock();

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

impl Drop for CollabGroup {
  fn drop(&mut self) {
    self.cancel.cancel();
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
