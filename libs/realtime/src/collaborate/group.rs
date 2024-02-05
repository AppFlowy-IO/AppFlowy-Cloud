use crate::collaborate::{CollabAccessControl, CollabBroadcast, CollabStoragePlugin, Subscription};
use crate::entities::RealtimeUser;
use anyhow::Error;
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use database::collab::CollabStorage;
use std::collections::HashMap;

use collab::core::collab_plugin::EncodedCollab;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::task::spawn_blocking;
use tokio::time::Instant;

use realtime_entity::collab_msg::CollabMessage;
use tracing::{debug, error, event, instrument, trace, warn};

pub struct CollabGroupCache<S, U, AC> {
  group_by_object_id: Arc<RwLock<HashMap<String, Arc<CollabGroup<U>>>>>,
  storage: Arc<S>,
  access_control: Arc<AC>,
}

impl<S, U, AC> CollabGroupCache<S, U, AC>
where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
{
  pub fn new(storage: Arc<S>, access_control: Arc<AC>) -> Self {
    Self {
      group_by_object_id: Arc::new(RwLock::new(HashMap::new())),
      storage,
      access_control,
    }
  }

  /// Performs a periodic check to remove groups based on the following conditions:
  /// 1. Groups without any subscribers.
  /// 2. Groups that have been inactive for a specified period of time.
  pub async fn tick(&self) {
    let mut inactive_group_ids = vec![];
    if let Ok(groups) = self.group_by_object_id.try_read() {
      for (object_id, group) in groups.iter() {
        if group.is_inactive().await {
          inactive_group_ids.push(object_id.clone());
          if inactive_group_ids.len() > 10 {
            break;
          }
        }
      }
    }

    if !inactive_group_ids.is_empty() {
      for object_id in inactive_group_ids {
        self.remove_group(&object_id).await;
      }
    }
  }

  pub async fn contains_user(&self, object_id: &str, user: &U) -> Result<bool, Error> {
    let group_by_object_id = self.group_by_object_id.try_read()?;
    if let Some(group) = group_by_object_id.get(object_id) {
      Ok(group.subscribers.try_read()?.get(user).is_some())
    } else {
      Ok(false)
    }
  }

  pub async fn remove_user(&self, object_id: &str, user: &U) -> Result<(), Error> {
    let group_by_object_id = self.group_by_object_id.try_read()?;
    if let Some(group) = group_by_object_id.get(object_id) {
      if let Some(subscriber) = group.subscribers.try_write()?.remove(user) {
        trace!("Remove subscriber: {}", subscriber.origin);
        subscriber.stop().await;
      }
    }
    Ok(())
  }

  pub async fn contains_group(&self, object_id: &str) -> Result<bool, Error> {
    let group_by_object_id = self.group_by_object_id.try_read()?;
    Ok(group_by_object_id.get(object_id).is_some())
  }

  pub async fn get_group(&self, object_id: &str) -> Option<Arc<CollabGroup<U>>> {
    self
      .group_by_object_id
      .try_read()
      .ok()?
      .get(object_id)
      .cloned()
  }

  #[instrument(skip(self))]
  pub async fn remove_group(&self, object_id: &str) {
    match self.group_by_object_id.try_write() {
      Ok(mut group_by_object_id) => {
        match group_by_object_id.remove(object_id) {
          None => {
            // The group should be exist, but it's not. This is an unexpected situation.
            error!("Group for object_id:{} not found", object_id);
          },
          Some(group) => {
            group.flush_collab().await;
          },
        }
        self.storage.remove_collab_cache(object_id).await;
      },
      Err(err) => error!("Failed to acquire write lock to remove group: {:?}", err),
    }
  }

  pub async fn create_group_if_need(
    &self,
    uid: i64,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) {
    match self.group_by_object_id.try_write() {
      Ok(mut group_by_object_id) => {
        if group_by_object_id.contains_key(object_id) {
          warn!("Group for object_id:{} already exists", object_id);
          return;
        }

        let group = self
          .init_group(uid, workspace_id, object_id, collab_type)
          .await;
        debug!("[realtime]: {} create group:{}", uid, object_id);
        group_by_object_id.insert(object_id.to_string(), group);
      },
      Err(err) => error!("Failed to acquire write lock to create group: {:?}", err),
    }
  }

  #[tracing::instrument(skip(self))]
  async fn init_group(
    &self,
    uid: i64,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Arc<CollabGroup<U>> {
    event!(
      tracing::Level::INFO,
      "Create new group for object_id:{}",
      object_id
    );
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
    collab.lock().add_plugin(Arc::new(plugin));
    event!(tracing::Level::INFO, "Init group collab:{}", object_id);
    collab.lock_arc().initialize().await;

    self
      .storage
      .cache_collab(object_id, Arc::downgrade(&collab))
      .await;
    group.observe_collab().await;
    group
  }

  #[allow(dead_code)]
  pub async fn number_of_groups(&self) -> usize {
    self.group_by_object_id.read().await.keys().len()
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
  pub subscribers: RwLock<HashMap<U, Subscription>>,

  pub modified_at: Arc<Mutex<Instant>>,
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
      modified_at,
    }
  }

  pub async fn observe_collab(&self) {
    self.broadcast.observe_collab_changes().await;
  }

  pub fn subscribe<Sink, Stream, E>(
    &self,
    subscriber_origin: CollabOrigin,
    sink: Sink,
    stream: Stream,
  ) -> Subscription
  where
    Sink: SinkExt<CollabMessage> + Send + Sync + Unpin + 'static,
    Stream: StreamExt<Item = Result<CollabMessage, E>> + Send + Sync + Unpin + 'static,
    <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error + Send + Sync,
    E: Into<Error> + Send + Sync + 'static,
  {
    self
      .broadcast
      .subscribe(subscriber_origin, sink, stream, self.modified_at.clone())
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
    self.subscribers.read().await.is_empty()
  }

  /// Check if the group is active. A group is considered active if it has at least one
  /// subscriber or has been modified within the last 10 minutes.
  pub async fn is_inactive(&self) -> bool {
    let modified_at = self.modified_at.lock().await;
    let is_timeout = modified_at.elapsed().as_secs() > self.timeout_secs();
    is_timeout && self.subscribers.read().await.is_empty()
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
      CollabType::Document => 10 * 60,
      CollabType::Database => 30 * 60,
      // WorkspaceDatabase, Folder, and UserAwareness share a longer timeout duration of 2 hours.
      CollabType::WorkspaceDatabase | CollabType::Folder | CollabType::UserAwareness => 2 * 60 * 60,
      // DatabaseRow timeout duration corrected to 1 hour.
      CollabType::DatabaseRow => 60 * 60,
    }
  }
}
