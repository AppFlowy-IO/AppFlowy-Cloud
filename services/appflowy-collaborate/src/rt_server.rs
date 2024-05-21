use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::Result;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tokio::sync::Notify;
use tokio::time::interval;
use tracing::{error, info, trace};

use access_control::collab::RealtimeAccessControl;
use collab_rt_entity::user::{RealtimeUser, UserDevice};
use collab_rt_entity::MessageByObjectId;
use collab_stream::client::CollabRedisStream;
use database::collab::CollabStorage;

use crate::client::client_msg_router::ClientMessageRouter;
use crate::command::{spawn_collaboration_command, CLCommandReceiver};
use crate::connect_state::ConnectState;
use crate::error::RealtimeError;
use crate::group::cmd::{GroupCommand, GroupCommandRunner, GroupCommandSender};
use crate::group::manager::GroupManager;
use crate::metrics::CollabMetricsCalculate;
use crate::state::RedisConnectionManager;
use crate::{spawn_metrics, CollabRealtimeMetrics, RealtimeClientWebsocketSink};

#[derive(Clone)]
pub struct CollaborationServer<S, AC> {
  /// Keep track of all collab groups
  group_manager: Arc<GroupManager<S, AC>>,
  connect_state: ConnectState,
  group_sender_by_object_id: Arc<DashMap<String, GroupCommandSender>>,
  access_control: Arc<AC>,
  storage: Arc<S>,
  #[allow(dead_code)]
  metrics: Arc<CollabRealtimeMetrics>,
  metrics_calculate: CollabMetricsCalculate,
}

impl<S, AC> CollaborationServer<S, AC>
where
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  pub async fn new(
    storage: Arc<S>,
    access_control: AC,
    metrics: Arc<CollabRealtimeMetrics>,
    command_recv: CLCommandReceiver,
    redis_connection_manager: RedisConnectionManager,
  ) -> Result<Self, RealtimeError> {
    if cfg!(feature = "collab-rt-multi-thread") {
      info!("CollaborationServer with multi-thread feature enabled");
    }

    let metrics_calculate = CollabMetricsCalculate::default();
    let connect_state = ConnectState::new();
    let access_control = Arc::new(access_control);
    let collab_stream = CollabRedisStream::new_with_connection_manager(redis_connection_manager);
    let group_manager = Arc::new(
      GroupManager::new(
        storage.clone(),
        access_control.clone(),
        metrics_calculate.clone(),
        collab_stream,
      )
      .await?,
    );
    let group_sender_by_object_id: Arc<DashMap<String, GroupCommandSender>> =
      Arc::new(Default::default());

    spawn_period_check_inactive_group(Arc::downgrade(&group_manager), &group_sender_by_object_id);

    spawn_collaboration_command(command_recv, &group_sender_by_object_id);

    spawn_metrics(&metrics, &metrics_calculate, &storage);

    Ok(Self {
      storage,
      group_manager,
      connect_state,
      group_sender_by_object_id,
      access_control,
      metrics,
      metrics_calculate,
    })
  }

  /// Handles a new user connection, replacing any existing connection for the same user.
  ///
  /// - Creates a new client stream for the connected user.
  /// - Replaces any existing user connection with the new one, signaling the old connection
  ///   if it's replaced.
  /// - Removes the old user connection from all collaboration groups.
  ///
  pub fn handle_new_connection(
    &self,
    connected_user: RealtimeUser,
    conn_sink: impl RealtimeClientWebsocketSink,
  ) -> Pin<Box<dyn Future<Output = Result<(), RealtimeError>>>> {
    let new_client_router = ClientMessageRouter::new(conn_sink);
    let group_manager = self.group_manager.clone();
    let connect_state = self.connect_state.clone();
    let metrics_calculate = self.metrics_calculate.clone();
    let storage = self.storage.clone();

    Box::pin(async move {
      storage
        .add_connected_user(connected_user.uid, &connected_user.device_id)
        .await;

      if let Some(old_user) = connect_state.handle_user_connect(connected_user, new_client_router) {
        // Remove the old user from all collaboration groups.
        group_manager.remove_user(&old_user).await;
      }
      metrics_calculate.connected_users.store(
        connect_state.number_of_connected_users() as i64,
        std::sync::atomic::Ordering::Relaxed,
      );
      Ok(())
    })
  }

  /// Handles a user's disconnection from the collaboration server.
  ///
  /// Steps:
  /// 1. Checks if the disconnecting user's session matches the stored session.
  ///    - If yes, proceeds with removal.
  ///    - If not, exits without action.
  /// 2. Removes the user from collaboration groups and client streams.
  pub fn handle_disconnect(
    &self,
    disconnect_user: RealtimeUser,
  ) -> Pin<Box<dyn Future<Output = Result<(), RealtimeError>>>> {
    let group_manager = self.group_manager.clone();
    let connect_state = self.connect_state.clone();
    let metrics_calculate = self.metrics_calculate.clone();
    let storage = self.storage.clone();

    Box::pin(async move {
      trace!("[realtime]: disconnect => {}", disconnect_user);
      let was_removed = connect_state.handle_user_disconnect(&disconnect_user);
      if was_removed.is_some() {
        storage
          .remove_connected_user(disconnect_user.uid, &disconnect_user.device_id)
          .await;

        metrics_calculate.connected_users.store(
          connect_state.number_of_connected_users() as i64,
          std::sync::atomic::Ordering::Relaxed,
        );

        group_manager.remove_user(&disconnect_user).await;
      }

      Ok(())
    })
  }

  #[inline]
  pub fn handle_client_message(
    &self,
    user: RealtimeUser,
    message_by_oid: MessageByObjectId,
  ) -> Pin<Box<dyn Future<Output = Result<(), RealtimeError>>>> {
    let group_sender_by_object_id = self.group_sender_by_object_id.clone();
    let client_msg_router_by_user = self.connect_state.client_message_routers.clone();
    let group_manager = self.group_manager.clone();
    let access_control = self.access_control.clone();

    Box::pin(async move {
      for (object_id, collab_messages) in message_by_oid {
        let old_sender = group_sender_by_object_id
          .get(&object_id)
          .map(|entry| entry.value().clone());

        let sender = match old_sender {
          Some(sender) => sender,
          None => match group_sender_by_object_id.entry(object_id.clone()) {
            Entry::Occupied(entry) => entry.get().clone(),
            Entry::Vacant(entry) => {
              let (new_sender, recv) = tokio::sync::mpsc::channel(2000);
              let notify = Arc::new(Notify::new());
              let runner = GroupCommandRunner {
                group_manager: group_manager.clone(),
                msg_router_by_user: client_msg_router_by_user.clone(),
                access_control: access_control.clone(),
                recv: Some(recv),
              };

              let object_id = entry.key().clone();
              let clone_notify = notify.clone();
              tokio::spawn(runner.run(object_id, clone_notify));
              entry.insert(new_sender.clone());

              // wait for the runner to be ready to handle the message.
              notify.notified().await;
              new_sender
            },
          },
        };

        let cloned_user = user.clone();
        // Create a new task to send a message to the group command runner without waiting for the
        // result. This approach is used to prevent potential issues with the actor's mailbox in
        // single-threaded runtimes (like actix-web actors). By spawning a task, the actor can
        // immediately proceed to process the next message.
        tokio::spawn(async move {
          let (tx, rx) = tokio::sync::oneshot::channel();
          match sender
            .send(GroupCommand::HandleClientCollabMessage {
              user: cloned_user,
              object_id,
              collab_messages,
              ret: tx,
            })
            .await
          {
            Ok(_) => {
              if let Ok(Err(err)) = rx.await {
                error!("Handle client collab message fail: {}", err);
              }
            },
            Err(err) => {
              // it should not happen. Because the receiver is always running before acquiring the sender.
              // Otherwise, the GroupCommandRunner might not be ready to handle the message.
              error!("Send message to group fail: {}", err);
            },
          }
        });
      }

      Ok(())
    })
  }

  pub fn get_user_by_device(&self, user_device: &UserDevice) -> Option<RealtimeUser> {
    self
      .connect_state
      .user_by_device
      .get(user_device)
      .map(|entry| entry.value().clone())
  }
}

fn spawn_period_check_inactive_group<S, AC>(
  weak_groups: Weak<GroupManager<S, AC>>,
  group_sender_by_object_id: &Arc<DashMap<String, GroupCommandSender>>,
) where
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  let mut interval = interval(Duration::from_secs(60));
  let cloned_group_sender_by_object_id = group_sender_by_object_id.clone();
  tokio::task::spawn_local(async move {
    loop {
      interval.tick().await;
      if let Some(groups) = weak_groups.upgrade() {
        let inactive_group_ids = groups.inactive_groups().await;
        for id in inactive_group_ids {
          cloned_group_sender_by_object_id.remove(&id);
        }
      } else {
        break;
      }
    }
  });
}

/// When the CollaborationServer operates within an actix-web actor, utilizing tokio::spawn for
/// task execution confines all tasks to the same thread, attributable to the actor's reliance on a
/// single-threaded Tokio runtime. To circumvent this limitation and enable task execution across
/// multiple threads, we've incorporated a multi-thread feature.
///
/// When appflowy-collaborate is deployed as a standalone service, we can use tokio multi-thread.
#[cfg(feature = "collab-rt-multi-thread")]
mod collaboration_runtime {
  use std::future::Future;
  use std::io;

  use lazy_static::lazy_static;
  use tokio::runtime;
  use tokio::runtime::Runtime;

  lazy_static! {
    pub(crate) static ref COLLAB_RUNTIME: Runtime = default_tokio_runtime().unwrap();
  }

  pub fn default_tokio_runtime() -> io::Result<Runtime> {
    runtime::Builder::new_multi_thread()
      .thread_name("collab-rt")
      .enable_io()
      .enable_time()
      .build()
  }

  pub(crate) fn spawn<T>(future: T) -> tokio::task::JoinHandle<T::Output>
  where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
  {
    COLLAB_RUNTIME.spawn(future)
  }
}
