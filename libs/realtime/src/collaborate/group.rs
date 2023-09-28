use crate::collaborate::{CollabBroadcast, CollabClientStream, CollabStoragePlugin, Subscription};

use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_define::CollabType;

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::iter::Take;
use std::pin::Pin;

use anyhow::Error;
use collab_sync_protocol::CollabMessage;
use parking_lot::Mutex;
use std::sync::{Arc, Weak};
use std::time::Duration;
use storage::collab::CollabStorage;
use tokio::sync::RwLock;

use crate::entities::{ClientMessage, Editing, RealtimeUser};
use tokio::task::spawn_blocking;
use tokio_retry::strategy::FixedInterval;
use tokio_retry::{Action, Condition, RetryIf};

use crate::error::RealtimeError;
use tracing::{error, trace, warn};

pub struct CollabGroupCache<S, U> {
  group_by_object_id: Arc<RwLock<HashMap<String, Arc<CollabGroup<U>>>>>,
  storage: S,
}

impl<S, U> CollabGroupCache<S, U>
where
  S: CollabStorage + Clone,
  U: RealtimeUser,
{
  pub fn new(storage: S) -> Self {
    Self {
      group_by_object_id: Arc::new(RwLock::new(HashMap::new())),
      storage,
    }
  }

  pub async fn contains_user(&self, object_id: &str, user: &U) -> Result<bool, Error> {
    let group_by_object_id = self.group_by_object_id.read().await;
    if let Some(group) = group_by_object_id.get(object_id) {
      Ok(group.subscribers.read().await.get(user).is_some())
    } else {
      Ok(false)
    }
  }

  pub async fn contains_group(&self, object_id: &str) -> Result<bool, Error> {
    let group_by_object_id = self.group_by_object_id.try_read()?;
    Ok(group_by_object_id.get(object_id).is_some())
  }

  pub async fn get_group(&self, object_id: &str) -> Option<Arc<CollabGroup<U>>> {
    self.group_by_object_id.read().await.get(object_id).cloned()
  }

  pub async fn remove_group(&self, object_id: &str) {
    match self.group_by_object_id.try_write() {
      Ok(mut group_by_object_id) => {
        group_by_object_id.remove(object_id);
      },
      Err(err) => error!("Failed to acquire write lock to remove group: {:?}", err),
    }
  }

  pub async fn create_group(
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
    tracing::trace!("Create new group for object_id:{}", object_id);
    let collab = MutexCollab::new(CollabOrigin::Server, object_id, vec![]);
    let broadcast = CollabBroadcast::new(object_id, collab.clone(), 10);
    let collab = Arc::new(collab.clone());

    // The lifecycle of the collab is managed by the group.
    let group = Arc::new(CollabGroup {
      collab: collab.clone(),
      broadcast,
      subscribers: Default::default(),
    });

    let plugin = CollabStoragePlugin::new(
      uid,
      workspace_id,
      collab_type,
      self.storage.clone(),
      Arc::downgrade(&group),
    );
    collab.lock().add_plugin(Arc::new(plugin));
    collab.async_initialize().await;

    self
      .storage
      .cache_collab(object_id, Arc::downgrade(&collab))
      .await;
    group
  }
}

/// A group used to manage a single [Collab] object
pub struct CollabGroup<U> {
  pub collab: Arc<MutexCollab>,

  /// A broadcast used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
  /// to subscribes.
  pub broadcast: CollabBroadcast,

  /// A list of subscribers to this group. Each subscriber will receive updates from the
  /// broadcast.
  pub subscribers: RwLock<HashMap<U, Subscription>>,
}

impl<U> CollabGroup<U>
where
  U: RealtimeUser,
{
  /// Mutate the [Collab] by the given closure
  pub fn get_mut_collab<F>(&self, f: F)
  where
    F: FnOnce(&Collab),
  {
    let collab = self.collab.lock();
    f(&collab);
  }

  pub async fn is_empty(&self) -> bool {
    self.subscribers.read().await.is_empty()
  }

  /// Flush the [Collab] to the storage.
  /// When there is no subscriber, perform the flush in a blocking task.
  pub fn save_collab(&self) {
    let collab = self.collab.clone();
    spawn_blocking(move || {
      collab.lock().flush();
    });
  }
}

pub(crate) struct SubscribeGroupIfNeedAction<'a, U, S> {
  pub(crate) client_msg: &'a ClientMessage<U>,
  pub(crate) groups: &'a Arc<CollabGroupCache<S, U>>,
  pub(crate) edit_collab_by_user: &'a Arc<Mutex<HashMap<U, HashSet<Editing>>>>,
  pub(crate) client_stream_by_user: &'a Arc<RwLock<HashMap<U, CollabClientStream>>>,
}

impl<'a, U, S> SubscribeGroupIfNeedAction<'a, U, S>
where
  U: RealtimeUser,
  S: CollabStorage,
{
  pub(crate) fn run(
    self,
  ) -> RetryIf<Take<FixedInterval>, SubscribeGroupIfNeedAction<'a, U, S>, RetryCondition<U>> {
    let weak_client_stream = Arc::downgrade(self.client_stream_by_user);
    let retry_strategy = FixedInterval::new(Duration::from_secs(2)).take(5);
    RetryIf::spawn(retry_strategy, self, RetryCondition(weak_client_stream))
  }
}

impl<'a, U, S> Action for SubscribeGroupIfNeedAction<'a, U, S>
where
  U: RealtimeUser,
  S: CollabStorage,
{
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + 'a>>;
  type Item = ();
  type Error = RealtimeError;

  fn run(&mut self) -> Self::Future {
    Box::pin(async {
      let object_id = self.client_msg.content.object_id();
      if !self.groups.contains_group(object_id).await? {
        // When create a group, the message must be the init sync message.
        match &self.client_msg.content {
          CollabMessage::ClientInit(client_init) => {
            let uid = client_init
              .origin
              .client_user_id()
              .ok_or(RealtimeError::UnexpectedData("The client user id is empty"))?;
            self
              .groups
              .create_group(
                uid,
                &client_init.workspace_id,
                object_id,
                client_init.collab_type.clone(),
              )
              .await;
          },
          _ => {
            return Err(RealtimeError::UnexpectedData(
              "The first message must be init sync message",
            ));
          },
        }
      }

      // If the client's stream is already subscribed to the collab group, return.
      if self
        .groups
        .contains_user(object_id, &self.client_msg.user)
        .await
        .unwrap_or(false)
      {
        return Ok(());
      }

      let origin = match self.client_msg.content.origin() {
        None => {
          error!("🔴The origin from client message is empty");
          &CollabOrigin::Empty
        },
        Some(client) => client,
      };
      match self
        .client_stream_by_user
        .write()
        .await
        .get_mut(&self.client_msg.user)
      {
        None => warn!("The client stream is not found"),
        Some(client_stream) => {
          if let Some(collab_group) = self.groups.get_group(object_id).await {
            collab_group
              .subscribers
              .write()
              .await
              .entry(self.client_msg.user.clone())
              .or_insert_with(|| {
                trace!(
                  "[💭Server]: {} subscribe group:{}",
                  self.client_msg.user,
                  self.client_msg.content.object_id()
                );

                self
                  .edit_collab_by_user
                  .lock()
                  .entry(self.client_msg.user.clone())
                  .or_default()
                  .insert(Editing {
                    object_id: object_id.to_string(),
                    origin: origin.clone(),
                  });

                let (sink, stream) = client_stream
                  .client_channel::<CollabMessage, _, _>(
                    object_id,
                    move |object_id, msg| msg.object_id() == object_id,
                    move |object_id, msg| msg.object_id == object_id,
                  )
                  .unwrap();

                collab_group
                  .broadcast
                  .subscribe(origin.clone(), sink, stream)
              });
          }
        },
      }

      Ok(())
    })
  }
}

pub struct RetryCondition<U>(pub Weak<RwLock<HashMap<U, CollabClientStream>>>);
impl<U> Condition<RealtimeError> for RetryCondition<U> {
  fn should_retry(&mut self, _error: &RealtimeError) -> bool {
    self.0.upgrade().is_some()
  }
}
