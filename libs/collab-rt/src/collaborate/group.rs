use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use dashmap::DashMap;
use std::rc::Rc;

use crate::collaborate::group_broadcast::{CollabBroadcast, Subscription};
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
  user_by_user_device: DashMap<String, RealtimeUser>,
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
      user_by_user_device: Default::default(),
    }
  }

  pub async fn encode_collab(&self) -> EncodedCollab {
    self.collab.lock().await.encode_collab_v1()
  }

  pub fn contains_user(&self, user: &RealtimeUser) -> bool {
    self.subscribers.contains_key(user)
  }

  pub async fn remove_user(&self, user: &RealtimeUser) {
    trace!("{} remove subscriber from group: {}", self.object_id, user);
    if let Some((_, mut old_sub)) = self.subscribers.remove(user) {
      old_sub.stop().await;
    }
  }

  pub fn user_count(&self) -> usize {
    self.subscribers.len()
  }

  /// Subscribes a new connection to the broadcast group for collaborative activities.
  ///
  /// This method establishes a new subscription for a user, represented by a `sink`/`stream` pair.
  /// These pairs implement the futures `Sink` and `Stream` protocols, facilitating real-time
  /// communication between the server and the client.
  ///
  /// # Parameters
  /// - `user`: Reference to the user initiating the subscription. Used for managing user-specific
  ///   subscriptions and ensuring unique subscriptions per user-device combination.
  /// - `subscriber_origin`: Identifies the origin of the subscription, used to prevent echoing
  ///   messages back to the sender.
  /// - `sink`: A `Sink` implementation used for sending collaboration changes to the client.
  /// - `stream`: A `Stream` implementation for receiving messages from the client.
  ///
  /// # Behavior
  /// - **Sink**: Utilized for forwarding any collaboration changes within the group to the client.
  ///   Ensures that updates are communicated in real-time.
  ///
  ///   Collaboration Group Changes
  ///               |
  ///               | (1) Detect Change
  ///               V
  ///   +---------------------------+
  ///   | Subscribe Function        |
  ///   +---------------------------+
  ///               |
  ///               | (2) Forward Update
  ///               V
  ///        +-------------+
  ///        |             |
  ///        | Sink        |-----> (To Client)
  ///        |             |
  ///        +-------------+
  ///
  /// - **Stream**: Processes incoming messages from the client. After processing, responses are
  ///   dispatched back to the client through the `sink`.
  ///        (From Client)
  ///             |
  ///             | (1) Receive Message
  ///             V
  ///        +-------------+
  ///        |             |
  ///        | Stream      |
  ///        |             |
  ///        +-------------+
  ///             |
  ///             | (2) Process Message
  ///             V
  ///   +---------------------------+
  ///   | Subscribe Function        |
  ///   +---------------------------+
  ///             |
  ///             | (3) Alter Document (if applicable)
  ///             V
  ///   Collaboration Group Updates (triggers Sink flow)
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
    // Remove the old user if it exists
    let user_device = user.user_device();
    if let Some((_, old)) = self.user_by_user_device.remove(&user_device) {
      trace!(
        "{} remove subscriber when resubscribing: {}",
        self.object_id,
        old
      );
      if let Some((_, mut old_sub)) = self.subscribers.remove(&old) {
        old_sub.stop().await;
      }
    }

    // create new subscription for new subscriber
    let sub = self.broadcast.subscribe(
      user,
      subscriber_origin,
      sink,
      stream,
      Rc::downgrade(&self.collab),
    );

    // insert the device for given user
    self
      .user_by_user_device
      .insert(user_device, (*user).clone());
    self.subscribers.insert((*user).clone(), sub);

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
