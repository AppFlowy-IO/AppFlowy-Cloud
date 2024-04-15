use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab_entity::CollabType;
use dashmap::DashMap;

use std::sync::Arc;

use crate::error::RealtimeError;
use crate::group::broadcast::{CollabBroadcast, Subscription};
use crate::group::persistence::GroupPersistence;
use crate::metrics::CollabMetricsCalculate;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::CollabMessage;
use collab_rt_entity::MessageByObjectId;
use database::collab::CollabStorage;

use crate::rt_server::rt_spawn;
use collab::core::collab::MutexCollab;
use futures_util::{SinkExt, StreamExt};

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use tokio::sync::mpsc;
use tracing::{event, trace};

/// A group used to manage a single [Collab] object
pub struct CollabGroup {
  pub workspace_id: String,
  pub object_id: String,
  collab: MutexCollab,
  collab_type: CollabType,
  /// A broadcast used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
  /// to subscribes.
  broadcast: CollabBroadcast,
  /// A list of subscribers to this group. Each subscriber will receive updates from the
  /// broadcast.
  subscribers: DashMap<RealtimeUser, Subscription>,
  metrics_calculate: CollabMetricsCalculate,
  destroy_group_tx: mpsc::Sender<MutexCollab>,
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
    collab: MutexCollab,
    metrics_calculate: CollabMetricsCalculate,
    storage: Arc<S>,
    is_new_collab: bool,
  ) -> Self
  where
    S: CollabStorage,
  {
    let edit_state = Arc::new(EditState::new(100, 360, is_new_collab));
    let broadcast = CollabBroadcast::new(&object_id, 10, edit_state.clone(), &collab).await;
    let (destroy_group_tx, rx) = mpsc::channel(1);

    rt_spawn(
      GroupPersistence::new(
        workspace_id.clone(),
        object_id.clone(),
        uid,
        storage,
        edit_state.clone(),
        collab.downgrade(),
        collab_type.clone(),
      )
      .run(rx),
    );

    Self {
      workspace_id,
      object_id,
      collab_type,
      collab,
      broadcast,
      subscribers: Default::default(),
      metrics_calculate,
      destroy_group_tx,
    }
  }

  pub async fn encode_collab(&self) -> Result<EncodedCollab, RealtimeError> {
    let encode_collab = self
      .collab
      .lock()
      .try_encode_collab_v1(|collab| self.collab_type.validate(collab))?;
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
      self.collab.downgrade(),
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
    if cfg!(debug_assertions) {
      modified_at.elapsed().as_secs() > 60 && self.subscribers.is_empty()
    } else {
      modified_at.elapsed().as_secs() > self.timeout_secs() && self.subscribers.is_empty()
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
      CollabType::Database | CollabType::DatabaseRow => 60 * 60, // 1 hour
      CollabType::WorkspaceDatabase | CollabType::Folder | CollabType::UserAwareness => 2 * 60 * 60, // 2 hours,
      CollabType::Empty => {
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
  is_new: AtomicBool,
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
    }
  }

  pub(crate) fn edit_count(&self) -> u32 {
    self.edit_counter.load(Ordering::SeqCst)
  }

  /// Increments the edit count and returns the new value.
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

  pub(crate) fn should_save_to_disk(&self) -> bool {
    let current_edit_count = self.edit_counter.load(Ordering::SeqCst);
    let prev_edit_count = self.prev_edit_count.load(Ordering::SeqCst);

    // If the collab is new, save it to disk and reset the flag
    if self.is_new.load(Ordering::SeqCst) {
      self.is_new.store(false, Ordering::SeqCst);
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
    let edit_state = EditState::new(10, 10, true);
    edit_state.increment_edit_count();
    assert!(edit_state.should_save_to_disk());
    edit_state.tick();

    for _ in 0..10 {
      edit_state.increment_edit_count();
    }
    assert!(edit_state.should_save_to_disk());
    assert!(edit_state.should_save_to_disk());
    edit_state.tick();
    assert!(!edit_state.should_save_to_disk());
  }
}
