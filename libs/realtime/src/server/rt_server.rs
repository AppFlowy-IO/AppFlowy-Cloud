use crate::entities::{
  ClientMessage, ClientStreamMessage, Connect, Disconnect, Editing, RealtimeMessage, RealtimeUser,
};
use crate::error::RealtimeError;
use crate::server::{RealtimeAccessControl, RealtimeMetrics};
use crate::util::channel_ext::UnboundedSenderSink;
use actix::{Actor, Context, Handler, ResponseFuture};
use anyhow::Result;
use collab::core::collab_plugin::EncodedCollab;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use database::collab::CollabStorage;

use realtime_entity::collab_msg::{ClientCollabMessage, CollabSinkMessage};
use realtime_entity::message::{MessageByObjectId, SystemMessage};
use std::collections::HashSet;

use crate::client::rt_client::RealtimeClientWebsocketSink;
use crate::server::collaborate::all_group::AllGroup;
use crate::server::collaborate::group_cmd::{GroupCommand, GroupCommandRunner, GroupCommandSender};
use std::sync::Arc;
use std::time::Duration;

use tokio::time::interval;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_stream::StreamExt;
use tracing::{error, event, info, instrument, trace, warn};

#[derive(Clone)]
pub struct RealtimeServer<S, U, AC> {
  #[allow(dead_code)]
  storage: Arc<S>,
  /// Keep track of all collab groups
  groups: Arc<AllGroup<S, U, AC>>,
  user_by_device: Arc<DashMap<UserDevice, U>>,
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
  client_stream_by_user: Arc<DashMap<U, CollabClientStream>>,
  group_sender_by_object_id: Arc<DashMap<String, GroupCommandSender<U>>>,
  access_control: Arc<AC>,
  #[allow(dead_code)]
  metrics: Arc<RealtimeMetrics>,
}

impl<S, U, AC> RealtimeServer<S, U, AC>
where
  S: CollabStorage,
  U: RealtimeUser,
  AC: RealtimeAccessControl,
{
  pub fn new(
    storage: Arc<S>,
    access_control: AC,
    metrics: Arc<RealtimeMetrics>,
    mut command_recv: RTCommandReceiver,
  ) -> Result<Self, RealtimeError> {
    let access_control = Arc::new(access_control);
    let groups = Arc::new(AllGroup::new(storage.clone(), access_control.clone()));
    let client_stream_by_user: Arc<DashMap<U, CollabClientStream>> = Default::default();
    let editing_collab_by_user = Default::default();
    let group_sender_by_object_id: Arc<DashMap<String, GroupCommandSender<U>>> =
      Arc::new(Default::default());

    let weak_groups = Arc::downgrade(&groups);
    let cloned_metrics = metrics.clone();
    let cloned_client_stream_by_user = client_stream_by_user.clone();
    let cloned_storage = storage.clone();
    let cloned_group_sender_by_object_id = group_sender_by_object_id.clone();

    tokio::spawn(async move {
      while let Some(cmd) = command_recv.recv().await {
        match cmd {
          RTCommand::GetEncodeCollab { object_id, ret } => {
            match cloned_group_sender_by_object_id.get(&object_id) {
              Some(sender) => {
                if let Err(err) = sender
                  .send(GroupCommand::EncodeCollab {
                    object_id: object_id.clone(),
                    ret,
                  })
                  .await
                {
                  error!("Send group command error: {}", err);
                }
              },
              None => {
                let _ = ret.send(None);
              },
            }
          },
        }
      }
    });

    let cloned_group_sender_by_object_id = group_sender_by_object_id.clone();
    tokio::task::spawn_local(async move {
      let mut interval = if cfg!(debug_assertions) {
        interval(Duration::from_secs(10))
      } else {
        interval(Duration::from_secs(60))
      };
      loop {
        interval.tick().await;
        if let Some(groups) = weak_groups.upgrade() {
          let inactive_group_ids = groups.tick().await;
          for id in inactive_group_ids {
            cloned_group_sender_by_object_id.remove(&id);
          }

          cloned_metrics.record_opening_collab_count(groups.number_of_groups().await);
          cloned_metrics.record_connected_users(cloned_client_stream_by_user.len());
          cloned_metrics
            .record_encode_collab_mem_hit_rate(cloned_storage.encode_collab_mem_hit_rate());
        } else {
          break;
        }
      }
    });

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

  #[inline]
  #[allow(clippy::too_many_arguments)]
  async fn process_client_collab_message(
    user: &U,
    group_sender_by_object_id: &Arc<DashMap<String, GroupCommandSender<U>>>,
    client_stream_by_user: &Arc<DashMap<U, CollabClientStream>>,
    groups: &Arc<AllGroup<S, U, AC>>,
    edit_collab_by_user: &Arc<DashMap<U, HashSet<Editing>>>,
    access_control: &Arc<AC>,
    object_id: String,
    collab_messages: Vec<ClientCollabMessage>,
  ) -> Result<(), RealtimeError> {
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
    Ok(())
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
      remove_user_from_group(user, groups, &editing).await;
    }
  }
}

impl<S, U, AC> Actor for RealtimeServer<S, U, AC>
where
  S: 'static + Unpin,
  U: RealtimeUser + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  type Context = Context<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    ctx.set_mailbox_capacity(3000);
  }
}

impl<S, U, AC> Handler<Connect<U>> for RealtimeServer<S, U, AC>
where
  U: RealtimeUser + Unpin,
  S: CollabStorage + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  type Result = ResponseFuture<Result<(), RealtimeError>>;

  fn handle(&mut self, new_conn: Connect<U>, _ctx: &mut Context<Self>) -> Self::Result {
    // User with the same id and same device will be replaced with the new connection [CollabClientStream]
    let client_stream = CollabClientStream::new(new_conn.socket);
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

      client_stream_by_user.insert(new_conn.user, client_stream);
      Ok(())
    })
  }
}

impl<S, U, AC> Handler<Disconnect<U>> for RealtimeServer<S, U, AC>
where
  U: RealtimeUser + Unpin,
  S: CollabStorage + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  type Result = ResponseFuture<Result<(), RealtimeError>>;
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
  fn handle(&mut self, msg: Disconnect<U>, _: &mut Context<Self>) -> Self::Result {
    trace!("[realtime]: disconnect => {}", msg.user);
    let groups = self.groups.clone();
    let client_stream_by_user = self.client_stream_by_user.clone();
    let editing_collab_by_user = self.editing_collab_by_user.clone();
    let session_id_by_user = self.session_id_by_user.clone();

    Box::pin(async move {
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
}

impl<S, U, AC> Handler<ClientMessage<U>> for RealtimeServer<S, U, AC>
where
  U: RealtimeUser + Unpin,
  S: CollabStorage + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  type Result = ResponseFuture<Result<(), RealtimeError>>;

  fn handle(&mut self, client_msg: ClientMessage<U>, _ctx: &mut Context<Self>) -> Self::Result {
    let ClientMessage { user, message } = client_msg;
    let group_sender_by_object_id = self.group_sender_by_object_id.clone();
    let client_stream_by_user = self.client_stream_by_user.clone();
    let groups = self.groups.clone();
    let edit_collab_by_user = self.editing_collab_by_user.clone();
    let access_control = self.access_control.clone();

    Box::pin(async move {
      match message.try_into_message_by_object_id() {
        Ok(map) => {
          for (oid, collab_messages) in map {
            if let Err(err) = Self::process_client_collab_message(
              &user,
              &group_sender_by_object_id,
              &client_stream_by_user,
              &groups,
              &edit_collab_by_user,
              &access_control,
              oid,
              collab_messages,
            )
            .await
            {
              error!("process collab message error: {}", err);
            }
          }
        },
        Err(err) => {
          if cfg!(debug_assertions) {
            error!("parse client message error: {}", err);
          }
        },
      }
      Ok(())
    })
  }
}

impl<S, U, AC> Handler<ClientStreamMessage> for RealtimeServer<S, U, AC>
where
  U: RealtimeUser + Unpin,
  S: CollabStorage + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  type Result = ResponseFuture<Result<(), RealtimeError>>;

  fn handle(&mut self, client_msg: ClientStreamMessage, _ctx: &mut Context<Self>) -> Self::Result {
    let ClientStreamMessage {
      uid,
      mut device_id,
      mut stream,
    } = client_msg;
    let group_sender_by_object_id = self.group_sender_by_object_id.clone();
    let user_by_device = self.user_by_device.clone();
    let client_stream_by_user = self.client_stream_by_user.clone();
    let groups = self.groups.clone();
    let edit_collab_by_user = self.editing_collab_by_user.clone();
    let access_control = self.access_control.clone();

    Box::pin(async move {
      if let Some(message) = stream.next().await {
        match message.try_into_message_by_object_id() {
          Ok(collab_messages) => {
            for (oid, collab_messages) in collab_messages {
              // client doesn't send the device_id through the http request header before the 0.4.6
              // so, try to get the device_id from messages.
              if device_id.is_empty() {
                if let Some(msg_device_id) = collab_messages.first().and_then(|v| v.device_id()) {
                  device_id = msg_device_id;
                }
              }

              let device_user = UserDevice::new(&device_id, uid);
              if let Some(entry) = user_by_device.get(&device_user) {
                if let Err(err) = Self::process_client_collab_message(
                  entry.value(),
                  &group_sender_by_object_id,
                  &client_stream_by_user,
                  &groups,
                  &edit_collab_by_user,
                  &access_control,
                  oid,
                  collab_messages,
                )
                .await
                {
                  error!("process collab message error: {}", err);
                }
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
      Ok(())
    })
  }
}

#[inline]
pub async fn broadcast_client_collab_message<U>(
  user: &U,
  object_id: String,
  collab_messages: Vec<ClientCollabMessage>,
  client_streams: &Arc<DashMap<U, CollabClientStream>>,
) where
  U: RealtimeUser,
{
  if let Some(client_stream) = client_streams.get(user) {
    trace!(
      "[realtime]: receive: uid:{} oid:{} {:?}",
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
      .send(RealtimeMessage::ClientCollabV1([pair].into()));
    if let Err(err) = err {
      warn!("Send user:{} message to group error: {}", user.uid(), err,);
      client_streams.remove(user);
    }
  }
}

/// Remove the user from the group and remove the group from the cache if the group is empty.
#[instrument(level = "debug", skip_all)]
async fn remove_user_from_group<S, U, AC>(
  user: &U,
  groups: &Arc<AllGroup<S, U, AC>>,
  editing: &Editing,
) where
  S: CollabStorage,
  U: RealtimeUser,
  AC: RealtimeAccessControl,
{
  let _ = groups.remove_user(&editing.object_id, user).await;
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

impl<S, U, AC> actix::Supervised for RealtimeServer<S, U, AC>
where
  S: 'static + Unpin,
  U: RealtimeUser + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  fn restarting(&mut self, _ctx: &mut Context<RealtimeServer<S, U, AC>>) {
    warn!("restarting");
  }
}

pub struct CollabClientStream {
  sink: RealtimeClientWebsocketSink,
  /// Used to receive messages from the collab server. The message will forward to the [CollabBroadcast] which
  /// will broadcast the message to all connected clients.
  ///
  /// The message flow:
  /// ClientSession(websocket) -> [RealtimeServer] -> [CollabClientStream] -> [CollabBroadcast] 1->* websocket(client)
  stream_tx: tokio::sync::broadcast::Sender<RealtimeMessage>,
}

impl CollabClientStream {
  pub fn new(sink: RealtimeClientWebsocketSink) -> Self {
    // When receive a new connection, create a new [ClientStream] that holds the connection's websocket
    let (stream_tx, _) = tokio::sync::broadcast::channel(1000);
    Self { sink, stream_tx }
  }

  /// Returns a [UnboundedSenderSink] and a [ReceiverStream] for the object_id.
  /// [Sink] will be used to receive changes from the collab object. Before receiving the changes, the sink_filter
  /// will be used to check if the client is allowed to receive the changes.
  ///
  /// [Stream] will be used to send changes to the collab object. Before sending the changes, the stream_filter
  /// will be used to check if the client is allowed to send the changes.
  ///
  pub fn client_channel<T, AC>(
    &mut self,
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
    tokio::spawn(async move {
      while let Some(msg) = client_sink_rx.recv().await {
        let result = sink_access_control
          .can_read_collab(&uid, &cloned_object_id)
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

    // forward the message to the stream that was subscribed by the broadcast group
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    tokio::spawn(async move {
      while let Some(Ok(realtime_msg)) = stream_rx.next().await {
        match realtime_msg.try_into_message_by_object_id() {
          Ok(messages_by_oid) => {
            for (object_id, messages) in messages_by_oid {
              let messages =
                Self::access_control(&uid, &object_id, &access_control, messages).await;
              if messages.is_empty() {
                continue;
              }
              if !messages.is_empty() {
                if tx.send([(object_id, messages)].into()).await.is_err() {
                  break;
                }
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
    uid: &i64,
    object_id: &str,
    access_control: &Arc<AC>,
    messages: Vec<ClientCollabMessage>,
  ) -> Vec<ClientCollabMessage>
  where
    AC: RealtimeAccessControl,
  {
    let can_write = access_control
      .can_write_collab(uid, object_id)
      .await
      .unwrap_or(false);

    let mut valid_messages = Vec::with_capacity(messages.len());
    for message in messages {
      if message.is_init_msg()
        && access_control
          .can_read_collab(uid, object_id)
          .await
          .unwrap_or(false)
      {
        valid_messages.push(message);
        continue;
      }

      if can_write {
        valid_messages.push(message);
      }
    }
    valid_messages
  }
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
struct UserDevice {
  device_id: String,
  uid: i64,
}

impl UserDevice {
  fn new(device_id: &str, uid: i64) -> Self {
    Self {
      device_id: device_id.to_string(),
      uid,
    }
  }
}

impl<T> From<&T> for UserDevice
where
  T: RealtimeUser,
{
  fn from(user: &T) -> Self {
    Self {
      device_id: user.device_id().to_string(),
      uid: user.uid(),
    }
  }
}

pub type RTCommandSender = tokio::sync::mpsc::Sender<RTCommand>;
pub type RTCommandReceiver = tokio::sync::mpsc::Receiver<RTCommand>;

pub type EncodeCollabSender = tokio::sync::oneshot::Sender<Option<EncodedCollab>>;
pub enum RTCommand {
  GetEncodeCollab {
    object_id: String,
    ret: EncodeCollabSender,
  },
}
