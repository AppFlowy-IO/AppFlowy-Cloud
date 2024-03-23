use crate::collaborate::all_group::AllGroup;
use crate::collaborate::group_cmd::{GroupCommand, GroupCommandRunner, GroupCommandSender};
use crate::command::{spawn_rt_command, RTCommandReceiver};
use crate::error::RealtimeError;
use crate::util::channel_ext::UnboundedSenderSink;
use crate::{spawn_metrics, CollabRealtimeMetrics, RealtimeAccessControl};

use anyhow::Result;
use collab_rt_entity::collab_msg::{ClientCollabMessage, CollabSinkMessage};
use collab_rt_entity::message::{MessageByObjectId, RealtimeMessage};
use collab_rt_entity::user::{Editing, RealtimeUser, UserDevice};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use database::collab::CollabStorage;
use std::collections::HashSet;
use std::future::Future;

use crate::connect_state::ConnectState;
use async_trait::async_trait;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_stream::StreamExt;
use tracing::{error, event, trace, warn};

#[derive(Clone)]
pub struct CollabRealtimeServer<S, AC> {
  /// Keep track of all collab groups
  groups: Arc<AllGroup<S, AC>>,
  connect_state: ConnectState,
  group_sender_by_object_id: Arc<DashMap<String, GroupCommandSender>>,
  access_control: Arc<AC>,
  /// Keep track of all object ids that a user is subscribed to
  editing_collab_by_user: Arc<DashMap<RealtimeUser, HashSet<Editing>>>,
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
    let groups = Arc::new(AllGroup::new(storage.clone(), access_control.clone()));
    let group_sender_by_object_id: Arc<DashMap<String, GroupCommandSender>> =
      Arc::new(Default::default());

    spawn_rt_command(command_recv, &group_sender_by_object_id);

    spawn_metrics(
      &group_sender_by_object_id,
      Arc::downgrade(&groups),
      &metrics,
      &connect_state.client_stream_by_user,
      &storage,
    );

    Ok(Self {
      groups,
      connect_state,
      group_sender_by_object_id,
      access_control,
      editing_collab_by_user: Arc::new(Default::default()),
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
    let new_client_stream = CollabClientStream::new(conn_sink);
    let groups = self.groups.clone();
    let connect_control = self.connect_state.clone();
    let editing_collab_by_user = self.editing_collab_by_user.clone();

    Box::pin(async move {
      if let Some(old_user) = connect_control.handle_user_connect(connected_user, new_client_stream)
      {
        // Remove the old user from all collaboration groups.
        remove_user_in_groups(&groups, &editing_collab_by_user, &old_user).await;
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
    let groups = self.groups.clone();
    let connect_control = self.connect_state.clone();
    let editing_collab_by_user = self.editing_collab_by_user.clone();

    Box::pin(async move {
      trace!("[realtime]: disconnect => {}", disconnect_user);
      let was_removed = connect_control.handle_user_disconnect(&disconnect_user);
      if was_removed.is_some() {
        remove_user_in_groups(&groups, &editing_collab_by_user, &disconnect_user).await;
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
    let client_stream_by_user = self.connect_state.client_stream_by_user.clone();
    let groups = self.groups.clone();
    let edit_collab_by_user = self.editing_collab_by_user.clone();
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
                all_groups: groups.clone(),
                client_stream_by_user: client_stream_by_user.clone(),
                edit_collab_by_user: edit_collab_by_user.clone(),
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

async fn remove_user_in_groups<S, AC>(
  groups: &Arc<AllGroup<S, AC>>,
  editing_collab_by_user: &Arc<DashMap<RealtimeUser, HashSet<Editing>>>,
  user: &RealtimeUser,
) where
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  let entry = editing_collab_by_user.remove(user);
  if let Some(entry) = entry {
    for editing in entry.1 {
      let _ = groups.remove_user(&editing.object_id, user).await;
      // Remove the user from the group and remove the group from the cache if the group is empty.
      if let Some(group) = groups.get_group(&editing.object_id).await {
        event!(
          tracing::Level::TRACE,
          "{}: Remove group subscriber:{}, Current group member: {}",
          &editing.object_id,
          editing.origin,
          group.user_count(),
        );
      }
    }
  }
}

#[async_trait]
pub trait RealtimeClientWebsocketSink: Send + Sync + 'static {
  fn do_send(&self, message: RealtimeMessage);
}

pub struct CollabClientStream {
  pub(crate) sink: Arc<dyn RealtimeClientWebsocketSink>,
  /// Used to receive messages from the collab server. The message will forward to the [CollabBroadcast] which
  /// will broadcast the message to all connected clients.
  ///
  /// The message flow:
  /// ClientSession(websocket) -> [CollabRealtimeServer] -> [CollabClientStream] -> [CollabBroadcast] 1->* websocket(client)
  pub(crate) stream_tx: tokio::sync::broadcast::Sender<RealtimeMessage>,
}

impl CollabClientStream {
  pub fn new(sink: impl RealtimeClientWebsocketSink) -> Self {
    // When receive a new connection, create a new [ClientStream] that holds the connection's websocket
    let (stream_tx, _) = tokio::sync::broadcast::channel(1000);
    Self {
      sink: Arc::new(sink),
      stream_tx,
    }
  }

  /// Returns a [UnboundedSenderSink] and a [ReceiverStream] for the object_id.
  /// [Sink] will be used to receive changes from the collab object.
  ///
  /// [Stream] will be used to send changes to the collab object.
  ///
  pub fn client_channel<T, AC>(
    &mut self,
    workspace_id: &str,
    user: &RealtimeUser,
    object_id: &str,
    access_control: Arc<AC>,
  ) -> (UnboundedSenderSink<T>, ReceiverStream<MessageByObjectId>)
  where
    T: Into<RealtimeMessage> + Send + Sync + 'static,
    AC: RealtimeAccessControl,
  {
    let client_ws_sink = self.sink.clone();
    let mut stream_rx = BroadcastStream::new(self.stream_tx.subscribe());
    let cloned_object_id = object_id.to_string();

    // Send the message to the connected websocket client. When the client receive the message,
    // it will apply the changes.
    let (client_sink_tx, mut client_sink_rx) = tokio::sync::mpsc::unbounded_channel::<T>();
    let sink_access_control = access_control.clone();
    let sink_workspace_id = workspace_id.to_string();
    let uid = user.uid;
    tokio::spawn(async move {
      while let Some(msg) = client_sink_rx.recv().await {
        let result = sink_access_control
          .can_read_collab(&sink_workspace_id, &uid, &cloned_object_id)
          .await;
        match result {
          Ok(is_allowed) => {
            if is_allowed {
              let rt_msg = msg.into();
              client_ws_sink.do_send(rt_msg);
            } else {
              trace!(
                "user:{} is not allow to observe {} changes",
                uid,
                cloned_object_id
              );
              // when then client is not allowed to receive messages
              tokio::time::sleep(Duration::from_secs(2)).await;
            }
          },
          Err(err) => {
            error!("user:{} fail to receive updates: {}", uid, err);
            tokio::time::sleep(Duration::from_secs(1)).await;
          },
        }
      }
    });
    let client_sink = UnboundedSenderSink::<T>::new(client_sink_tx);

    let cloned_object_id = object_id.to_string();
    let stream_workspace_id = workspace_id.to_string();
    let user = user.clone();
    // stream_rx continuously receive messages from the websocket client and then
    // forward the message to the subscriber which is the broadcast channel [CollabBroadcast].
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    tokio::spawn(async move {
      while let Some(Ok(realtime_msg)) = stream_rx.next().await {
        match realtime_msg.transform() {
          Ok(messages_by_oid) => {
            for (msg_oid, original_messages) in messages_by_oid {
              if cloned_object_id != msg_oid {
                continue;
              }

              let (valid_messages, invalid_message) = Self::access_control(
                &stream_workspace_id,
                &user.uid,
                &msg_oid,
                &access_control,
                original_messages,
              )
              .await;
              trace!(
                "{} receive client:{}, device:{}, message: valid:{} invalid:{}",
                msg_oid,
                user.uid,
                user.device_id,
                valid_messages.len(),
                invalid_message.len()
              );

              if valid_messages.is_empty() {
                continue;
              }

              // if tx.send return error, it means the client is disconnected from the group
              if let Err(err) = tx.send([(msg_oid, valid_messages)].into()).await {
                trace!(
                  "{} send message to user:{} stream fail with error: {}, break the loop",
                  cloned_object_id,
                  user.user_device(),
                  err,
                );
                return;
              }
            }
          },
          Err(err) => {
            if cfg!(debug_assertions) {
              error!("parse client message error: {}", err);
            }
          },
        }
      }
    });
    let client_stream = ReceiverStream::new(rx);
    (client_sink, client_stream)
  }

  async fn access_control<AC>(
    workspace_id: &str,
    uid: &i64,
    object_id: &str,
    access_control: &Arc<AC>,
    messages: Vec<ClientCollabMessage>,
  ) -> (Vec<ClientCollabMessage>, Vec<ClientCollabMessage>)
  where
    AC: RealtimeAccessControl,
  {
    let can_write = access_control
      .can_write_collab(workspace_id, uid, object_id)
      .await
      .unwrap_or(false);

    let mut valid_messages = Vec::with_capacity(messages.len());
    let mut invalid_messages = Vec::with_capacity(messages.len());
    let can_read = access_control
      .can_read_collab(workspace_id, uid, object_id)
      .await
      .unwrap_or(false);

    for message in messages {
      if message.is_client_init_sync() && can_read {
        valid_messages.push(message);
        continue;
      }

      if can_write {
        valid_messages.push(message);
      } else {
        invalid_messages.push(message);
      }
    }
    (valid_messages, invalid_messages)
  }
}

#[inline]
pub async fn broadcast_client_collab_message(
  user: &RealtimeUser,
  object_id: String,
  collab_messages: Vec<ClientCollabMessage>,
  client_streams: &Arc<DashMap<RealtimeUser, CollabClientStream>>,
) {
  if let Some(client_stream) = client_streams.get(user) {
    trace!(
      "[realtime]: receive: uid:{} oid:{} msg ids: {:?}",
      user.uid,
      object_id,
      collab_messages
        .iter()
        .map(|v| v.msg_id())
        .collect::<Vec<_>>()
    );
    let pair = (object_id, collab_messages);
    let err = client_stream
      .stream_tx
      .send(RealtimeMessage::ClientCollabV2([pair].into()));
    if let Err(err) = err {
      warn!("Send user:{} message to group error: {}", user.uid, err,);
      client_streams.remove(user);
    }
  }
}
