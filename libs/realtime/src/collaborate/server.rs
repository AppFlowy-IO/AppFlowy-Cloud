use crate::entities::{ClientMessage, Connect, Disconnect, Editing, RealtimeMessage, RealtimeUser};
use crate::error::{RealtimeError, StreamError};
use anyhow::Result;

use actix::{Actor, Context, Handler, ResponseFuture};
use futures_util::future::BoxFuture;
use parking_lot::Mutex;
use realtime_entity::collab_msg::CollabMessage;
use std::collections::{HashMap, HashSet};

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::time::interval;

use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_stream::StreamExt;
use tracing::{error, event, info, instrument, trace};

use crate::client::ClientWSSink;
use crate::collaborate::group::CollabGroupCache;
use crate::collaborate::permission::CollabAccessControl;
use crate::collaborate::retry::{CollabUserMessage, SubscribeGroupIfNeed};
use crate::util::channel_ext::UnboundedSenderSink;
use database::collab::CollabStorage;

#[derive(Clone)]
pub struct CollabServer<S, U, AC> {
  #[allow(dead_code)]
  storage: Arc<S>,
  /// Keep track of all collab groups
  groups: Arc<CollabGroupCache<S, U, AC>>,
  /// Keep track of all object ids that a user is subscribed to
  editing_collab_by_user: Arc<Mutex<HashMap<U, HashSet<Editing>>>>,
  /// Keep track of all client streams
  client_stream_by_user: Arc<RwLock<HashMap<U, CollabClientStream>>>,
  access_control: Arc<AC>,
}

impl<S, U, AC> CollabServer<S, U, AC>
where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
{
  pub fn new(storage: Arc<S>, access_control: AC) -> Result<Self, RealtimeError> {
    let access_control = Arc::new(access_control);
    let groups = Arc::new(CollabGroupCache::new(
      storage.clone(),
      access_control.clone(),
    ));
    let edit_collab_by_user = Arc::new(Mutex::new(HashMap::new()));

    // Periodically check the collab groups
    let weak_group = Arc::downgrade(&groups);
    tokio::spawn(async move {
      let mut interval = interval(Duration::from_secs(60));
      loop {
        interval.tick().await;
        match weak_group.upgrade() {
          Some(groups) => groups.tick().await,
          None => break,
        }
      }
    });

    Ok(Self {
      storage,
      groups,
      editing_collab_by_user: edit_collab_by_user,
      client_stream_by_user: Default::default(),
      access_control,
    })
  }
}

async fn remove_user<S, U, AC>(
  groups: &Arc<CollabGroupCache<S, U, AC>>,
  editing_collab_by_user: &Arc<Mutex<HashMap<U, HashSet<Editing>>>>,
  user: &U,
) where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
{
  let editing_set = editing_collab_by_user.lock().remove(user);
  if let Some(editing_set) = editing_set {
    info!("Remove user from group: {}", user);
    for editing in editing_set {
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
    let stream = CollabClientStream::new(ClientWSSink(new_conn.socket));
    let groups = self.groups.clone();
    let client_stream_by_user = self.client_stream_by_user.clone();
    let editing_collab_by_user = self.editing_collab_by_user.clone();

    Box::pin(async move {
      trace!("[realtime]: new connection => {} ", new_conn.user);
      remove_user(&groups, &editing_collab_by_user, &new_conn.user).await;
      if let Some(old_stream) = client_stream_by_user
        .write()
        .await
        .insert(new_conn.user, stream)
      {
        old_stream.disconnect();
      }

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
  fn handle(&mut self, msg: Disconnect<U>, _: &mut Context<Self>) -> Self::Result {
    trace!("[realtime]: disconnect => {}", msg.user);
    let groups = self.groups.clone();
    let client_stream_by_user = self.client_stream_by_user.clone();
    let editing_collab_by_user = self.editing_collab_by_user.clone();
    Box::pin(async move {
      remove_user(&groups, &editing_collab_by_user, &msg.user).await;
      if client_stream_by_user
        .write()
        .await
        .remove(&msg.user)
        .is_some()
      {
        info!("Remove user stream: {}", &msg.user);
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

    trace!(
      "Receive message from client:{} message:{}",
      user.uid(),
      message
    );
    match message {
      RealtimeMessage::Collab(collab_message) => {
        let client_stream_by_user = self.client_stream_by_user.clone();
        let groups = self.groups.clone();
        let edit_collab_by_user = self.editing_collab_by_user.clone();
        let permission_service = self.access_control.clone();

        Box::pin(async move {
          let msg = CollabUserMessage {
            user: &user,
            collab_message: &collab_message,
          };
          SubscribeGroupIfNeed {
            collab_user_message: &msg,
            groups: &groups,
            edit_collab_by_user: &edit_collab_by_user,
            client_stream_by_user: &client_stream_by_user,
            access_control: &permission_service,
          }
          .run()
          .await?;

          broadcast_message(&user, &collab_message, &client_stream_by_user).await;
          Ok(())
        })
      },
      _ => Box::pin(async { Ok(()) }),
    }
  }
}

#[inline]
async fn broadcast_message<U>(
  user: &U,
  collab_message: &CollabMessage,
  client_streams: &Arc<RwLock<HashMap<U, CollabClientStream>>>,
) where
  U: RealtimeUser,
{
  if let Some(client_stream) = client_streams.read().await.get(user) {
    trace!("[realtime]: receives collab message: {}", collab_message);
    match client_stream
      .stream_tx
      .send(Ok(RealtimeMessage::Collab(collab_message.clone())))
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
  groups: &Arc<CollabGroupCache<S, U, AC>>,
  editing: &Editing,
) where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
{
  groups.remove_user(&editing.object_id, user).await;
  if let Some(group) = groups.get_group(&editing.object_id).await {
    event!(
      tracing::Level::INFO,
      "Remove group subscriber: {}",
      editing.origin
    );

    event!(
      tracing::Level::DEBUG,
      "{}: Group member: {}. member ids: {:?}",
      &editing.object_id,
      group.subscribers.read().await.len(),
      group
        .subscribers
        .read()
        .await
        .values()
        .map(|value| value.origin.to_string())
        .collect::<Vec<_>>(),
    );

    // Destroy the group if the group is empty
    let should_remove = group.is_empty().await;
    if should_remove {
      group.flush_collab();
      event!(tracing::Level::INFO, "Remove group: {}", editing.object_id);
      groups.remove_group(&editing.object_id).await;
    }
  }
}

impl<S, U, AC> actix::Supervised for CollabServer<S, U, AC>
where
  S: 'static + Unpin,
  U: RealtimeUser + Unpin,
  AC: CollabAccessControl + Unpin,
{
  fn restarting(&mut self, _ctx: &mut Context<CollabServer<S, U, AC>>) {
    tracing::warn!("restarting");
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
    self.sink.do_send(RealtimeMessage::ServerKickedOff);
  }
}
