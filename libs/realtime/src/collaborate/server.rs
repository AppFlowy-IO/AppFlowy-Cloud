use crate::client::ClientWSSink;
use crate::collaborate::group::{GroupCommand, GroupCommandRunner, GroupControlCommandSender};
use crate::collaborate::group_control::CollabGroupControl;
use crate::collaborate::permission::CollabAccessControl;
use crate::collaborate::RealtimeMetrics;
use crate::entities::{
  ClientMessage, ClientStreamMessage, Connect, Disconnect, Editing, RealtimeMessage, RealtimeUser,
};
use crate::error::{RealtimeError, StreamError};
use crate::util::channel_ext::UnboundedSenderSink;
use actix::{Actor, Context, Handler, ResponseFuture};
use anyhow::Result;
use dashmap::DashMap;
use database::collab::CollabStorage;
use futures_util::future::BoxFuture;
use realtime_entity::collab_msg::CollabMessage;
use realtime_entity::message::SystemMessage;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_stream::StreamExt;
use tracing::{error, event, info, instrument, trace, warn};

#[derive(Clone)]
pub struct CollabServer<S, U, AC> {
  #[allow(dead_code)]
  storage: Arc<S>,
  /// Keep track of all collab groups
  groups: Arc<CollabGroupControl<S, U, AC>>,
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
  /// Keep track of all client streams
  client_stream_by_user: Arc<DashMap<U, CollabClientStream>>,
  group_sender_by_object_id: Arc<DashMap<String, GroupControlCommandSender<U>>>,
  access_control: Arc<AC>,
  #[allow(dead_code)]
  metrics: Arc<RealtimeMetrics>,
}

impl<S, U, AC> CollabServer<S, U, AC>
where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
{
  pub fn new(
    storage: Arc<S>,
    access_control: AC,
    metrics: Arc<RealtimeMetrics>,
  ) -> Result<Self, RealtimeError> {
    let access_control = Arc::new(access_control);
    let groups = Arc::new(CollabGroupControl::new(
      storage.clone(),
      access_control.clone(),
    ));
    let client_stream_by_user: Arc<DashMap<U, CollabClientStream>> = Default::default();
    let editing_collab_by_user = Default::default();
    let group_sender_by_object_id: Arc<DashMap<String, GroupControlCommandSender<U>>> =
      Arc::new(Default::default());

    let weak_groups = Arc::downgrade(&groups);
    let cloned_metrics = metrics.clone();
    let cloned_client_stream_by_user = client_stream_by_user.clone();
    let cloned_storage = storage.clone();
    let cloned_group_sender_by_object_id = group_sender_by_object_id.clone();
    tokio::spawn(async move {
      let mut interval = interval(Duration::from_secs(60));
      loop {
        interval.tick().await;
        if let Some(groups) = weak_groups.upgrade() {
          cloned_metrics.record_opening_collab_count(groups.number_of_groups().await);
          cloned_metrics.record_connected_users(cloned_client_stream_by_user.len());
          cloned_metrics.record_mem_cache_usage(cloned_storage.mem_usage());

          let inactive_group_ids = groups.tick().await;
          for id in inactive_group_ids {
            cloned_group_sender_by_object_id.remove(&id);
          }
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

  fn process_realtime_message(
    user: U,
    group_sender_by_object_id: Arc<DashMap<String, GroupControlCommandSender<U>>>,
    client_stream_by_user: Arc<DashMap<U, CollabClientStream>>,
    groups: Arc<CollabGroupControl<S, U, AC>>,
    edit_collab_by_user: Arc<DashMap<U, HashSet<Editing>>>,
    access_control: Arc<AC>,
    realtime_msg: RealtimeMessage,
  ) -> Pin<Box<impl Future<Output = Result<(), RealtimeError>>>> {
    Box::pin(async move {
      trace!("Receive client:{} message:{}", user.uid(), realtime_msg);
      match realtime_msg {
        RealtimeMessage::Collab(collab_message) => {
          let old_sender = group_sender_by_object_id
            .get(collab_message.object_id())
            .map(|entry| entry.value().clone());

          let sender = match old_sender {
            Some(sender) => sender,
            None => {
              let (new_sender, recv) = tokio::sync::mpsc::channel(1000);
              let runner = GroupCommandRunner {
                group_control: groups.clone(),
                client_stream_by_user: client_stream_by_user.clone(),
                edit_collab_by_user: edit_collab_by_user.clone(),
                access_control: access_control.clone(),
                recv: Some(recv),
              };
              tokio::task::spawn_local(runner.run());
              group_sender_by_object_id
                .insert(collab_message.object_id().to_string(), new_sender.clone());
              new_sender
            },
          };

          if let Err(err) = sender
            .send(GroupCommand::HandleCollabMessage {
              user,
              collab_message,
            })
            .await
          {
            error!("Send message to group error: {}", err);
          }
          Ok(())
        },
        _ => {
          warn!("Receive unsupported message: {}", realtime_msg);
          Ok(())
        },
      }
    })
  }
}

async fn remove_user<S, U, AC>(
  groups: &Arc<CollabGroupControl<S, U, AC>>,
  editing_collab_by_user: &Arc<DashMap<U, HashSet<Editing>>>,
  user: &U,
) where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
{
  let entry = editing_collab_by_user.remove(user);
  if let Some(entry) = entry {
    for editing in entry.1 {
      remove_user_from_group(user, groups, &editing).await;
    }
  }
}

impl<S, U, AC> Actor for CollabServer<S, U, AC>
where
  S: 'static + Unpin,
  U: RealtimeUser + Unpin,
  AC: CollabAccessControl + Unpin,
{
  type Context = Context<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    ctx.set_mailbox_capacity(3000);
  }
}

impl<S, U, AC> Handler<Connect<U>> for CollabServer<S, U, AC>
where
  U: RealtimeUser + Unpin,
  S: CollabStorage + Unpin,
  AC: CollabAccessControl + Unpin,
{
  type Result = ResponseFuture<Result<(), RealtimeError>>;

  fn handle(&mut self, new_conn: Connect<U>, _ctx: &mut Context<Self>) -> Self::Result {
    // User with the same id and same device will be replaced with the new connection [CollabClientStream]
    let client_stream = CollabClientStream::new(ClientWSSink(new_conn.socket));
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
          info!("same user connect again, remove the stream: {}", &old_user);
          old_stream.disconnect();
        }

        // when a new connection is established, remove the old connection from all groups
        remove_user(&groups, &editing_collab_by_user, &old_user).await;
      }

      client_stream_by_user.insert(new_conn.user, client_stream);
      Ok(())
    })
  }
}

impl<S, U, AC> Handler<Disconnect<U>> for CollabServer<S, U, AC>
where
  U: RealtimeUser + Unpin,
  S: CollabStorage + Unpin,
  AC: CollabAccessControl + Unpin,
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

impl<S, U, AC> Handler<ClientMessage<U>> for CollabServer<S, U, AC>
where
  U: RealtimeUser + Unpin,
  S: CollabStorage + Unpin,
  AC: CollabAccessControl + Unpin,
{
  type Result = ResponseFuture<Result<(), RealtimeError>>;

  fn handle(&mut self, client_msg: ClientMessage<U>, _ctx: &mut Context<Self>) -> Self::Result {
    let ClientMessage { user, message } = client_msg;
    let group_sender_by_object_id = self.group_sender_by_object_id.clone();
    let client_stream_by_user = self.client_stream_by_user.clone();
    let groups = self.groups.clone();
    let edit_collab_by_user = self.editing_collab_by_user.clone();
    let access_control = self.access_control.clone();
    Self::process_realtime_message(
      user,
      group_sender_by_object_id,
      client_stream_by_user,
      groups,
      edit_collab_by_user,
      access_control,
      message,
    )
  }
}

impl<S, U, AC> Handler<ClientStreamMessage> for CollabServer<S, U, AC>
where
  U: RealtimeUser + Unpin,
  S: CollabStorage + Unpin,
  AC: CollabAccessControl + Unpin,
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
        // client doesn't send the device_id through the http request header before the 0.4.6
        // so, try to get the device_id from the message
        if device_id.is_empty() {
          if let Some(msg_device_id) = message.device_id() {
            device_id = msg_device_id;
          }
        }
        let device_user = UserDevice { device_id, uid };
        let entry = user_by_device.get(&device_user);
        match entry {
          None => Err(RealtimeError::UserNotFound(format!(
            "Can't find the user:{} device_id:{} from client stream message",
            uid, device_user.device_id
          ))),
          Some(entry) => {
            Self::process_realtime_message(
              entry.value().clone(),
              group_sender_by_object_id,
              client_stream_by_user,
              groups,
              edit_collab_by_user,
              access_control,
              message,
            )
            .await
          },
        }
      } else {
        Ok(())
      }
    })
  }
}

#[inline]
pub async fn broadcast_message<U>(
  user: &U,
  collab_message: CollabMessage,
  client_streams: &Arc<DashMap<U, CollabClientStream>>,
) where
  U: RealtimeUser,
{
  if let Some(client_stream) = client_streams.get(user) {
    trace!("[realtime]: receives collab message: {}", collab_message);
    match client_stream
      .stream_tx
      .send(Ok(RealtimeMessage::Collab(collab_message)))
    {
      Ok(_) => {},
      Err(e) => error!("send error: {}", e),
    }
  }
}

/// Remove the user from the group and remove the group from the cache if the group is empty.
#[instrument(level = "debug", skip_all)]
async fn remove_user_from_group<S, U, AC>(
  user: &U,
  groups: &Arc<CollabGroupControl<S, U, AC>>,
  editing: &Editing,
) where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
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

impl<S, U, AC> actix::Supervised for CollabServer<S, U, AC>
where
  S: 'static + Unpin,
  U: RealtimeUser + Unpin,
  AC: CollabAccessControl + Unpin,
{
  fn restarting(&mut self, _ctx: &mut Context<CollabServer<S, U, AC>>) {
    warn!("restarting");
  }
}

pub struct CollabClientStream {
  sink: ClientWSSink,
  /// Used to receive messages from the collab server. The message will forward to the [CollabBroadcast] which
  /// will broadcast the message to all connected clients.
  ///
  /// The message flow:
  /// ClientSession(websocket) -> [CollabServer] -> [CollabClientStream] -> [CollabBroadcast] 1->* websocket(client)
  pub(crate) stream_tx: tokio::sync::broadcast::Sender<Result<RealtimeMessage, StreamError>>,
}

impl CollabClientStream {
  pub fn new(sink: ClientWSSink) -> Self {
    // When receive a new connection, create a new [ClientStream] that holds the connection's websocket
    let (stream_tx, _) = tokio::sync::broadcast::channel(1000);
    Self { sink, stream_tx }
  }

  /// Returns a [UnboundedSenderSink] and a [ReceiverStream] for the object_id.
  #[allow(clippy::type_complexity)]
  pub fn client_channel<T, SinkFilter, StreamFilter>(
    &mut self,
    object_id: &str,
    sink_filter: SinkFilter,
    stream_filter: StreamFilter,
  ) -> (
    UnboundedSenderSink<T>,
    ReceiverStream<Result<CollabMessage, StreamError>>,
  )
  where
    T: Into<RealtimeMessage> + Send + Sync + 'static,
    SinkFilter: Fn(&str, &T) -> BoxFuture<'static, bool> + Sync + Send + 'static,
    StreamFilter: Fn(&str, &CollabMessage) -> BoxFuture<'static, bool> + Sync + Send + 'static,
  {
    let client_ws_sink = self.sink.clone();
    let mut stream_rx = BroadcastStream::new(self.stream_tx.subscribe());
    let cloned_object_id = object_id.to_string();

    // Send the message to the connected websocket client
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<T>();
    tokio::task::spawn(async move {
      while let Some(msg) = rx.recv().await {
        let can_sink = sink_filter(&cloned_object_id, &msg).await;
        if can_sink {
          // Send the message to websocket client actor
          client_ws_sink.do_send(msg.into());
        } else {
          // when then client is not allowed to receive the message
          tokio::time::sleep(Duration::from_secs(2)).await;
        }
      }
    });
    let client_forward_sink = UnboundedSenderSink::<T>::new(tx);

    // forward the message to the stream that was subscribed by the broadcast group, which will
    // send the messages to all connected clients using the client_forward_sink
    let cloned_object_id = object_id.to_string();
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    tokio::spawn(async move {
      while let Some(Ok(Ok(RealtimeMessage::Collab(msg)))) = stream_rx.next().await {
        if stream_filter(&cloned_object_id, &msg).await {
          let _ = tx.send(Ok(msg)).await;
        } else {
          // when then client is not allowed to receive the message
          tokio::time::sleep(Duration::from_secs(2)).await;
        }
      }
    });
    let client_forward_stream = ReceiverStream::new(rx);

    // When broadcast group write a message to the client_forward_sink, the message will be forwarded
    // to the client's websocket sink, which will then send the message to the connected client
    //
    // When receiving a message from the client_forward_stream, it will send the message to the broadcast
    // group. The message will be broadcast to all connected clients.
    (client_forward_sink, client_forward_stream)
  }

  pub fn disconnect(&self) {
    self
      .sink
      .do_send(RealtimeMessage::System(SystemMessage::KickOff));
  }
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
struct UserDevice {
  device_id: String,
  uid: i64,
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
