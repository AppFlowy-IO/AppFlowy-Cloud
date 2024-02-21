use crate::collaborate::{CollabAccessControl, CollabBroadcast, CollabStoragePlugin, Subscription};
use crate::entities::RealtimeUser;
use anyhow::Error;
use collab::core::collab::MutexCollab;
use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use dashmap::DashMap;
use database::collab::CollabStorage;
use futures_util::{SinkExt, StreamExt};
use realtime_entity::collab_msg::CollabMessage;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::spawn_blocking;
use tokio::time::Instant;
use tracing::{debug, error, event, instrument, trace};

pub struct CollabGroupControl<S, U, AC> {
  group_by_object_id: Arc<DashMap<String, Arc<CollabGroup<U>>>>,
  storage: Arc<S>,
  access_control: Arc<AC>,
}

impl<S, U, AC> CollabGroupControl<S, U, AC>
where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
{
  pub fn new(storage: Arc<S>, access_control: Arc<AC>) -> Self {
    Self {
      group_by_object_id: Arc::new(DashMap::new()),
      storage,
      access_control,
    }
  }

  /// Performs a periodic check to remove groups based on the following conditions:
  /// 1. Groups without any subscribers.
  /// 2. Groups that have been inactive for a specified period of time.
  pub async fn tick(&self) -> Vec<String> {
    let mut inactive_group_ids = vec![];
    for entry in self.group_by_object_id.iter() {
      let (object_id, group) = (entry.key(), entry.value());
      if group.is_inactive().await {
        inactive_group_ids.push(object_id.clone());
        if inactive_group_ids.len() > 5 {
          break;
        }
      }
    }

    if !inactive_group_ids.is_empty() {
      for object_id in &inactive_group_ids {
        self.remove_group(object_id).await;
      }
    }
    inactive_group_ids
  }

  pub async fn contains_user(&self, object_id: &str, user: &U) -> bool {
    if let Some(entry) = self.group_by_object_id.get(object_id) {
      entry.value().contains_user(user)
    } else {
      false
    }
  }

  pub async fn remove_user(&self, object_id: &str, user: &U) -> Result<(), Error> {
    if let Some(entry) = self.group_by_object_id.get(object_id) {
      let group = entry.value();
      group.remove_user(user).await;
    }
    Ok(())
  }

  pub async fn contains_group(&self, object_id: &str) -> bool {
    self.group_by_object_id.get(object_id).is_some()
  }

  pub async fn get_group(&self, object_id: &str) -> Option<Arc<CollabGroup<U>>> {
    self
      .group_by_object_id
      .get(object_id)
      .map(|v| v.value().clone())
  }

  #[instrument(skip(self))]
  pub async fn remove_group(&self, object_id: &str) {
    let entry = self.group_by_object_id.remove(object_id);

    if let Some(entry) = entry {
      let group = entry.1;
      group.stop().await;
      group.flush_collab().await;
    } else {
      // Log error if the group doesn't exist
      error!("Group for object_id:{} not found", object_id);
    }

    self.storage.remove_collab_cache(object_id).await;
  }

  pub async fn create_group(
    &self,
    uid: i64,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) {
    let group = self
      .init_group(uid, workspace_id, object_id, collab_type)
      .await;
    debug!("[realtime]: {} create group:{}", uid, object_id);
    self.group_by_object_id.insert(object_id.to_string(), group);
  }

  #[tracing::instrument(level = "trace", skip(self))]
  async fn init_group(
    &self,
    uid: i64,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Arc<CollabGroup<U>> {
    event!(tracing::Level::TRACE, "New group:{}", object_id);
    let collab = MutexCollab::new(CollabOrigin::Server, object_id, vec![]);
    let broadcast = CollabBroadcast::new(object_id, collab.clone(), 10);
    let collab = Arc::new(collab.clone());

    // The lifecycle of the collab is managed by the group.
    let group = Arc::new(CollabGroup::new(
      collab_type.clone(),
      collab.clone(),
      broadcast,
    ));
    let plugin = CollabStoragePlugin::new(
      uid,
      workspace_id,
      collab_type,
      self.storage.clone(),
      Arc::downgrade(&group),
      self.access_control.clone(),
    );
    collab.lock().add_plugin(Box::new(plugin));
    event!(tracing::Level::TRACE, "Init group collab:{}", object_id);
    collab.lock_arc().initialize().await;

    self
      .storage
      .cache_collab(object_id, Arc::downgrade(&collab))
      .await;
    group.observe_collab().await;
    group
  }

  pub async fn number_of_groups(&self) -> usize {
    self.group_by_object_id.len()
  }
}

/// A group used to manage a single [Collab] object
pub struct CollabGroup<U> {
  pub collab: Arc<MutexCollab>,
  collab_type: CollabType,
  /// A broadcast used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
  /// to subscribes.
  broadcast: CollabBroadcast,
  /// A list of subscribers to this group. Each subscriber will receive updates from the
  /// broadcast.
  subscribers: DashMap<U, Subscription>,
  user_by_user_device: DashMap<String, U>,
  pub modified_at: Arc<Mutex<Instant>>,
}

impl<U> Drop for CollabGroup<U> {
  fn drop(&mut self) {
    trace!("Drop collab group:{}", self.collab.lock().object_id);
  }
}

impl<U> CollabGroup<U>
where
  U: RealtimeUser,
{
  pub fn new(
    collab_type: CollabType,
    collab: Arc<MutexCollab>,
    broadcast: CollabBroadcast,
  ) -> Self {
    let modified_at = Arc::new(Mutex::new(Instant::now()));
    Self {
      collab_type,
      collab,
      broadcast,
      subscribers: Default::default(),
      user_by_user_device: Default::default(),
      modified_at,
    }
  }

  pub async fn observe_collab(&self) {
    self.broadcast.observe_collab_changes().await;
  }

  pub fn contains_user(&self, user: &U) -> bool {
    self.subscribers.contains_key(user)
  }

  pub async fn remove_user(&self, user: &U) {
    if let Some((_, mut old_sub)) = self.subscribers.remove(user) {
      old_sub.stop().await;
    }
  }

  pub fn user_count(&self) -> usize {
    self.subscribers.len()
  }

  pub fn unsubscribe(&self, user: &U) {
    if let Some(subscription) = self.subscribers.remove(user) {
      let mut subscriber = subscription.1;
      tokio::spawn(async move {
        subscriber.stop().await;
      });
    }
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
  pub async fn subscribe<Sink, Stream, E>(
    &self,
    user: &U,
    subscriber_origin: CollabOrigin,
    sink: Sink,
    stream: Stream,
  ) where
    Sink: SinkExt<CollabMessage> + Clone + Send + Sync + Unpin + 'static,
    Stream: StreamExt<Item = Result<CollabMessage, E>> + Send + Sync + Unpin + 'static,
    <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error + Send + Sync,
    E: Into<Error> + Send + Sync + 'static,
  {
    let sub = self
      .broadcast
      .subscribe(subscriber_origin, sink, stream, self.modified_at.clone());

    // Remove the old user if it exists
    let user_device = user.user_device();
    if let Some((_, old)) = self.user_by_user_device.remove(&user_device) {
      if let Some((_, mut old_sub)) = self.subscribers.remove(&old) {
        old_sub.stop().await;
      }
    }

    self
      .user_by_user_device
      .insert(user_device, (*user).clone());
    self.subscribers.insert((*user).clone(), sub);
  }

  /// Mutate the [Collab] by the given closure
  pub fn get_mut_collab<F>(&self, f: F)
  where
    F: FnOnce(&Collab),
  {
    let collab = self.collab.lock();
    f(&collab);
  }

  pub fn encode_v1(&self) -> EncodedCollab {
    self.collab.lock().encode_collab_v1()
  }

  pub async fn is_empty(&self) -> bool {
    self.subscribers.is_empty()
  }

  /// Check if the group is active. A group is considered active if it has at least one
  /// subscriber or has been modified within the last 10 minutes.
  pub async fn is_inactive(&self) -> bool {
    let modified_at = self.modified_at.lock().await;
    if cfg!(debug_assertions) {
      modified_at.elapsed().as_secs() > 60
    } else {
      modified_at.elapsed().as_secs() > self.timeout_secs()
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
    let collab = self.collab.clone();
    let _ = spawn_blocking(move || {
      collab.lock().flush();
    })
    .await;
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
