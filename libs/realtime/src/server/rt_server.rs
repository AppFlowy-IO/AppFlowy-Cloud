use crate::entities::{Connect, Disconnect, RealtimeMessage};
use crate::error::RealtimeError;
use crate::server::collaborate::all_group::AllGroup;
use crate::server::collaborate::group_cmd::{GroupCommand, GroupCommandRunner, GroupCommandSender};
use crate::server::command::{spawn_rt_command, RTCommandReceiver};
use crate::server::{spawn_metrics, RealtimeAccessControl, RealtimeMetrics};
use crate::util::channel_ext::UnboundedSenderSink;

use anyhow::Result;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use database::collab::CollabStorage;
use realtime_entity::collab_msg::{ClientCollabMessage, CollabSinkMessage};
use realtime_entity::message::{MessageByObjectId, SystemMessage};
use realtime_entity::user::{Editing, RealtimeUser, UserDevice};
use std::collections::HashSet;
use std::future::Future;

use crate::client::rt_client::RealtimeClientWebsocketSinkImpl;
use async_trait::async_trait;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_stream::StreamExt;
use tracing::{error, event, info, trace, warn};

#[derive(Clone)]
pub struct RealtimeServer<S, U, AC, CS> {
  #[allow(dead_code)]
  storage: Arc<S>,
  /// Keep track of all collab groups
  groups: Arc<AllGroup<S, U, AC>>,
  pub(crate) user_by_device: Arc<DashMap<UserDevice, U>>,
  /// This map stores the session IDs for users currently connected to the server.
  /// The user's identifier [U] is used as the key, and their corresponding session ID is the value.
  ///
  /// When a user disconnects, their session ID is retrieved using their user identifier [U].
  /// This session ID is then compared with the session ID provided in the [Disconnect] message.
  /// If the two session IDs differ, it indicates that the user has established a new connection
  /// to the server since the stored session ID was last updated.
  ///
  session_id_by_user: Arc<DashMap<U, String>>,
  /// Keep track of all object ids that a user is subscribed to
  editing_collab_by_user: Arc<DashMap<U, HashSet<Editing>>>,
  /// Maintains a record of all client streams. A client stream associated with a user may be terminated for the following reasons:
  /// 1. User disconnection.
  /// 2. Server closes the connection due to a ping/pong timeout.
  client_stream_by_user: Arc<DashMap<U, CollabClientStream<CS>>>,
  group_sender_by_object_id: Arc<DashMap<String, GroupCommandSender<U>>>,
  access_control: Arc<AC>,
  #[allow(dead_code)]
  metrics: Arc<RealtimeMetrics>,
}

impl<S, U, AC, CS> RealtimeServer<S, U, AC, CS>
where
  S: CollabStorage,
  U: RealtimeUser,
  AC: RealtimeAccessControl,
  CS: RealtimeClientWebsocketSink,
{
  pub fn new(
    storage: Arc<S>,
    access_control: AC,
    metrics: Arc<RealtimeMetrics>,
    command_recv: RTCommandReceiver,
  ) -> Result<Self, RealtimeError> {
    let access_control = Arc::new(access_control);
    let groups = Arc::new(AllGroup::new(storage.clone(), access_control.clone()));
    let client_stream_by_user: Arc<DashMap<U, CollabClientStream<CS>>> = Default::default();
    let editing_collab_by_user = Default::default();
    let group_sender_by_object_id: Arc<DashMap<String, GroupCommandSender<U>>> =
      Arc::new(Default::default());

    spawn_rt_command(command_recv, &group_sender_by_object_id);

    spawn_metrics(
      &group_sender_by_object_id,
      Arc::downgrade(&groups),
      &metrics,
      &client_stream_by_user,
      &storage,
    );

    Ok(Self {
      storage,
      groups,
      user_by_device: Default::default(),
      session_id_by_user: Default::default(),
      editing_collab_by_user,
      client_stream_by_user,
      group_sender_by_object_id,
      access_control,
      metrics,
    })
  }

  pub(crate) fn handle_new_connection(
    &self,
    new_conn: Connect<U>,
  ) -> Pin<Box<dyn Future<Output = Result<(), RealtimeError>>>> {
    // User with the same id and same device will be replaced with the new connection [CollabClientStream]
    let new_client_stream =
      CollabClientStream::new(RealtimeClientWebsocketSinkImpl(new_conn.socket));
    let groups = self.groups.clone();
    let user_by_device = self.user_by_device.clone();
    let client_stream_by_user = self.client_stream_by_user.clone();
    let editing_collab_by_user = self.editing_collab_by_user.clone();
    let user_by_session_id = self.session_id_by_user.clone();

    Box::pin(async move {
      trace!("[realtime]: new connection => {} ", new_conn.user);
      user_by_session_id.insert(new_conn.user.clone(), new_conn.session_id);

      let old_user = user_by_device.insert(UserDevice::from(&new_conn.user), new_conn.user.clone());
      if let Some(old_user) = old_user {
        if let Some(value) = client_stream_by_user.remove(&old_user) {
          let old_stream = value.1;
          info!(
            "same user connect with the same device again, remove old stream: {}",
            &old_user
          );
          old_stream
            .sink
            .do_send(RealtimeMessage::System(SystemMessage::DuplicateConnection));
        }

        // when a new connection is established, remove the old connection from all groups
        remove_user(&groups, &editing_collab_by_user, &old_user).await;
      }

      client_stream_by_user.insert(new_conn.user, new_client_stream);
      Ok(())
    })
  }

  /// Handles the disconnection of a user from the collaboration server.
  ///
  /// Upon receiving a `Disconnect` message, the method performs the following actions:
  /// 1. Attempts to acquire a read lock on `session_id_by_user` to compare the stored session ID
  ///    with the session ID in the `Disconnect` message.
  ///    - If the session IDs match, it proceeds to remove the user from groups and client streams.
  ///    - If the session IDs do not match, indicating the user has reconnected with a new session,
  ///      no action is taken and the function returns.
  /// 2. Removes the user from the collaboration groups and client streams, if applicable.
  ///
  pub(crate) fn handle_disconnect(
    &self,
    msg: Disconnect<U>,
  ) -> Pin<Box<dyn Future<Output = Result<(), RealtimeError>>>> {
    let groups = self.groups.clone();
    let client_stream_by_user = self.client_stream_by_user.clone();
    let editing_collab_by_user = self.editing_collab_by_user.clone();
    let session_id_by_user = self.session_id_by_user.clone();

    Box::pin(async move {
      trace!("[realtime]: disconnect => {}", msg.user);
      // If the user has reconnected with a new session, the session id will be different.
      // So do not remove the user from groups and client streams
      if let Some(entry) = session_id_by_user.get(&msg.user) {
        if entry.value() != &msg.session_id {
          return Ok(());
        }
      }

      remove_user(&groups, &editing_collab_by_user, &msg.user).await;
      if client_stream_by_user.remove(&msg.user).is_some() {
        info!("remove client stream: {}", &msg.user);
      }
      Ok(())
    })
  }

  #[inline]
  pub(crate) fn handle_client_message(
    &self,
    user: U,
    message_by_oid: MessageByObjectId,
  ) -> Pin<Box<dyn Future<Output = Result<(), RealtimeError>>>> {
    let group_sender_by_object_id = self.group_sender_by_object_id.clone();
    let client_stream_by_user = self.client_stream_by_user.clone();
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
              let (new_sender, recv) = tokio::sync::mpsc::channel(1000);
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
}

async fn remove_user<S, U, AC>(
  groups: &Arc<AllGroup<S, U, AC>>,
  editing_collab_by_user: &Arc<DashMap<U, HashSet<Editing>>>,
  user: &U,
) where
  S: CollabStorage,
  U: RealtimeUser,
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
pub trait RealtimeClientWebsocketSink: Clone + Send + Sync + 'static {
  fn do_send(&self, message: RealtimeMessage);
}

pub struct CollabClientStream {
  sink: Box<dyn RealtimeClientWebsocketSink>,
  /// Used to receive messages from the collab server. The message will forward to the [CollabBroadcast] which
  /// will broadcast the message to all connected clients.
  ///
  /// The message flow:
  /// ClientSession(websocket) -> [RealtimeServer] -> [CollabClientStream] -> [CollabBroadcast] 1->* websocket(client)
  stream_tx: tokio::sync::broadcast::Sender<RealtimeMessage>,
}

impl CollabClientStream {
  pub fn new(sink: impl RealtimeClientWebsocketSink) -> Self {
    // When receive a new connection, create a new [ClientStream] that holds the connection's websocket
    let (stream_tx, _) = tokio::sync::broadcast::channel(1000);
    Self { sink, stream_tx }
  }

  /// Returns a [UnboundedSenderSink] and a [ReceiverStream] for the object_id.
  /// [Sink] will be used to receive changes from the collab object.
  ///
  /// [Stream] will be used to send changes to the collab object.
  ///
  pub fn client_channel<T, AC>(
    &mut self,
    workspace_id: &str,
    uid: i64,
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
    // forward the message to the stream that was subscribed by the broadcast group
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
                &uid,
                &msg_oid,
                &access_control,
                original_messages,
              )
              .await;
              trace!(
                "{} receive message: valid:{} invalid:{}",
                msg_oid,
                valid_messages.len(),
                invalid_message.len()
              );

              if valid_messages.is_empty() {
                continue;
              }
              if tx.send([(msg_oid, valid_messages)].into()).await.is_err() {
                break;
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
pub async fn broadcast_client_collab_message<U, CS>(
  user: &U,
  object_id: String,
  collab_messages: Vec<ClientCollabMessage>,
  client_streams: &Arc<DashMap<U, CollabClientStream<CS>>>,
) where
  U: RealtimeUser,
{
  if let Some(client_stream) = client_streams.get(user) {
    trace!(
      "[realtime]: receive: uid:{} oid:{} msg ids: {:?}",
      user.uid(),
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
      warn!("Send user:{} message to group error: {}", user.uid(), err,);
      client_streams.remove(user);
    }
  }
}
