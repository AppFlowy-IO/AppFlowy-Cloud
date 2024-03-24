use crate::client_msg_router::{ClientMessageRouter, RealtimeClientWebsocketSink};
use crate::collaborate::group_cmd::{GroupCommand, GroupCommandRunner, GroupCommandSender};
use crate::collaborate::group_manager::GroupManager;
use crate::command::{spawn_rt_command, RTCommandReceiver};
use crate::connect_state::ConnectState;
use crate::error::RealtimeError;
use crate::{spawn_metrics, CollabRealtimeMetrics, RealtimeAccessControl};
use anyhow::Result;
use collab_rt_entity::message::MessageByObjectId;
use collab_rt_entity::user::{RealtimeUser, UserDevice};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use database::collab::CollabStorage;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tracing::{error, trace};

#[derive(Clone)]
pub struct CollabRealtimeServer<S, AC> {
  /// Keep track of all collab groups
  group_manager: Arc<GroupManager<S, AC>>,
  connect_state: ConnectState,
  group_sender_by_object_id: Arc<DashMap<String, GroupCommandSender>>,
  access_control: Arc<AC>,
  #[allow(dead_code)]
  metrics: Arc<CollabRealtimeMetrics>,
}

impl<S, AC> CollabRealtimeServer<S, AC>
where
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  pub fn new(
    storage: Arc<S>,
    access_control: AC,
    metrics: Arc<CollabRealtimeMetrics>,
    command_recv: RTCommandReceiver,
  ) -> Result<Self, RealtimeError> {
    let connect_state = ConnectState::new();
    let access_control = Arc::new(access_control);
    let group_manager = Arc::new(GroupManager::new(storage.clone(), access_control.clone()));
    let group_sender_by_object_id: Arc<DashMap<String, GroupCommandSender>> =
      Arc::new(Default::default());

    spawn_rt_command(command_recv, &group_sender_by_object_id);

    spawn_metrics(
      &group_sender_by_object_id,
      Arc::downgrade(&group_manager),
      &metrics,
      &connect_state.client_message_routers,
      &storage,
    );

    Ok(Self {
      group_manager,
      connect_state,
      group_sender_by_object_id,
      access_control,
      metrics,
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

    Box::pin(async move {
      if let Some(old_user) = connect_state.handle_user_connect(connected_user, new_client_router) {
        // Remove the old user from all collaboration groups.
        group_manager.remove_user(&old_user).await;
      }
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
    let connect_control = self.connect_state.clone();

    Box::pin(async move {
      trace!("[realtime]: disconnect => {}", disconnect_user);
      let was_removed = connect_control.handle_user_disconnect(&disconnect_user);
      if was_removed.is_some() {
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
              let runner = GroupCommandRunner {
                group_manager: group_manager.clone(),
                client_msg_router_by_user: client_msg_router_by_user.clone(),
                access_control: access_control.clone(),
                recv: Some(recv),
              };

              let object_id = entry.key().clone();
              tokio::task::spawn_local(runner.run(object_id));
              entry.insert(new_sender.clone());
              new_sender
            },
          },
        };

        if let Err(err) = sender
          .send(GroupCommand::HandleClientCollabMessage {
            user: user.clone(),
            object_id,
            collab_messages,
          })
          .await
        {
          error!("Send message to group error: {}", err);
        }
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
