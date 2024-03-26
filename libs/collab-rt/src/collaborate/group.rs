use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use dashmap::DashMap;
use std::rc::Rc;

use crate::collaborate::group_broadcast::{CollabBroadcast, Subscription};
use crate::metrics::CollabMetricsCalculate;

use collab_rt_entity::collab_msg::CollabMessage;
use collab_rt_entity::message::MessageByObjectId;
use collab_rt_entity::user::RealtimeUser;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tracing::trace;

/// A group used to manage a single [Collab] object
pub struct CollabGroup {
  pub workspace_id: String,
  pub object_id: String,
  collab: Rc<Mutex<Collab>>,
  collab_type: CollabType,
  /// A broadcast used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
  /// to subscribes.
  broadcast: CollabBroadcast,
  /// A list of subscribers to this group. Each subscriber will receive updates from the
  /// broadcast.
  subscribers: DashMap<RealtimeUser, Subscription>,
  metrics_calculate: CollabMetricsCalculate,
}

impl Drop for CollabGroup {
  fn drop(&mut self) {
    trace!("Drop collab group:{}", self.object_id);
  }
}

impl CollabGroup {
  pub async fn new(
    workspace_id: String,
    object_id: String,
    collab_type: CollabType,
    mut collab: Collab,
    metrics_calculate: CollabMetricsCalculate,
  ) -> Self {
    let broadcast = CollabBroadcast::new(&object_id, 10);
    broadcast.observe_collab_changes(&mut collab).await;
    Self {
      workspace_id,
      object_id,
      collab_type,
      collab: Rc::new(Mutex::new(collab)),
      broadcast,
      subscribers: Default::default(),
      metrics_calculate,
    }
  }

  pub async fn encode_collab(&self) -> EncodedCollab {
    self.collab.lock().await.encode_collab_v1()
  }

  pub fn contains_user(&self, user: &RealtimeUser) -> bool {
    self.subscribers.contains_key(user)
  }

  pub async fn remove_user(&self, user: &RealtimeUser) {
    if let Some((_, mut old_sub)) = self.subscribers.remove(user) {
      trace!("{} remove subscriber from group: {}", self.object_id, user);
      tokio::spawn(async move {
        old_sub.stop().await;
      });
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
      Rc::downgrade(&self.collab),
      self.metrics_calculate.clone(),
    );

    if let Some(mut old) = self.subscribers.insert((*user).clone(), sub) {
      old.stop().await;
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
  }

  /// Flush the [Collab] to the storage.
  /// When there is no subscriber, perform the flush in a blocking task.
  pub async fn flush_collab(&self) {
    self.collab.lock().await.flush();
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
    }
  }
}
