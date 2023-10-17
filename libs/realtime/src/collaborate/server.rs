use crate::entities::{ClientMessage, Connect, Disconnect, Editing, RealtimeMessage, RealtimeUser};
use crate::error::{RealtimeError, StreamError};
use anyhow::Result;

use actix::{Actor, Context, Handler, ResponseFuture};
use futures_util::future::BoxFuture;
use parking_lot::Mutex;
use realtime_entity::collab_msg::CollabMessage;
use std::collections::{HashMap, HashSet};

use std::sync::Arc;

use tokio::sync::RwLock;

use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_stream::StreamExt;
use tracing::{info, trace};

use crate::client::ClientWSSink;
use crate::collaborate::group::CollabGroupCache;
use crate::collaborate::permission::CollabPermission;
use crate::collaborate::retry::SubscribeGroupIfNeed;
use crate::util::channel_ext::UnboundedSenderSink;
use database::collab::CollabStorage;

#[derive(Clone)]
pub struct CollabServer<S, U, P> {
  #[allow(dead_code)]
  storage: S,
  /// Keep track of all collab groups
  groups: Arc<CollabGroupCache<S, U>>,
  /// Keep track of all object ids that a user is subscribed to
  editing_collab_by_user: Arc<Mutex<HashMap<U, HashSet<Editing>>>>,
  /// Keep track of all client streams
  client_stream_by_user: Arc<RwLock<HashMap<U, CollabClientStream>>>,
  permission_service: Arc<P>,
}

impl<S, U, P> CollabServer<S, U, P>
where
  S: CollabStorage + Clone,
  U: RealtimeUser,
  P: CollabPermission,
{
  pub fn new(storage: S, permission_service: Arc<P>) -> Result<Self, RealtimeError> {
    let groups = Arc::new(CollabGroupCache::new(storage.clone()));
    let edit_collab_by_user = Arc::new(Mutex::new(HashMap::new()));
    Ok(Self {
      storage,
      groups,
      editing_collab_by_user: edit_collab_by_user,
      client_stream_by_user: Default::default(),
      permission_service,
    })
  }
}

async fn remove_user<S, U>(
  groups: &Arc<CollabGroupCache<S, U>>,
  client_stream_by_user: &Arc<RwLock<HashMap<U, CollabClientStream>>>,
  editing_collab_by_user: &Arc<Mutex<HashMap<U, HashSet<Editing>>>>,
  user: &U,
) where
  S: CollabStorage + Clone,
  U: RealtimeUser,
{
  if client_stream_by_user.write().await.remove(user).is_some() {
    info!("Remove user stream: {}", user);
  }

  let editing_set = editing_collab_by_user.lock().remove(user);
  if let Some(editing_set) = editing_set {
    info!("Remove user from group: {}", user);
    for editing in editing_set {
      remove_user_from_group(user, groups, &editing).await;
    }
  }
}

impl<S, U, P> Actor for CollabServer<S, U, P>
where
  S: 'static + Unpin,
  U: RealtimeUser + Unpin,
  P: CollabPermission + Unpin,
{
  type Context = Context<Self>;
}

impl<S, U, P> Handler<Connect<U>> for CollabServer<S, U, P>
where
  U: RealtimeUser + Unpin,
  S: CollabStorage + Unpin,
  P: CollabPermission + Unpin,
{
  type Result = ResponseFuture<Result<(), RealtimeError>>;

  fn handle(&mut self, new_conn: Connect<U>, _ctx: &mut Context<Self>) -> Self::Result {
    let stream = CollabClientStream::new(ClientWSSink(new_conn.socket));
    let groups = self.groups.clone();
    let client_stream_by_user = self.client_stream_by_user.clone();
    let editing_collab_by_user = self.editing_collab_by_user.clone();

    Box::pin(async move {
      trace!("[💭Server]: new connection => {} ", new_conn.user);
      remove_user(
        &groups,
        &client_stream_by_user,
        &editing_collab_by_user,
        &new_conn.user,
      )
      .await;

      client_stream_by_user
        .write()
        .await
        .insert(new_conn.user, stream);
      Ok(())
    })
  }
}

impl<S, U, P> Handler<Disconnect<U>> for CollabServer<S, U, P>
where
  U: RealtimeUser + Unpin,
  S: CollabStorage + Unpin,
  P: CollabPermission + Unpin,
{
  type Result = ResponseFuture<Result<(), RealtimeError>>;
  fn handle(&mut self, msg: Disconnect<U>, _: &mut Context<Self>) -> Self::Result {
    trace!("[💭Server]: disconnect => {}", msg.user);
    let groups = self.groups.clone();
    let client_stream_by_user = self.client_stream_by_user.clone();
    let editing_collab_by_user = self.editing_collab_by_user.clone();
    Box::pin(async move {
      remove_user(
        &groups,
        &client_stream_by_user,
        &editing_collab_by_user,
        &msg.user,
      )
      .await;
      Ok(())
    })
  }
}

impl<S, U, P> Handler<ClientMessage<U>> for CollabServer<S, U, P>
where
  U: RealtimeUser + Unpin,
  S: CollabStorage + Unpin,
  P: CollabPermission + Unpin,
{
  type Result = ResponseFuture<Result<(), RealtimeError>>;

  fn handle(&mut self, client_msg: ClientMessage<U>, _ctx: &mut Context<Self>) -> Self::Result {
    let client_stream_by_user = self.client_stream_by_user.clone();
    let groups = self.groups.clone();
    let edit_collab_by_user = self.editing_collab_by_user.clone();
    let permission_service = self.permission_service.clone();

    Box::pin(async move {
      SubscribeGroupIfNeed {
        client_msg: &client_msg,
        groups: &groups,
        edit_collab_by_user: &edit_collab_by_user,
        client_stream_by_user: &client_stream_by_user,
        permission_service: &permission_service,
      }
      .run()
      .await?;

      broadcast_message(&client_msg, &client_stream_by_user).await;
      Ok(())
    })
  }
}

#[inline]
async fn broadcast_message<U>(
  client_msg: &ClientMessage<U>,
  client_streams: &Arc<RwLock<HashMap<U, CollabClientStream>>>,
) where
  U: RealtimeUser,
{
  if let Some(client_stream) = client_streams.read().await.get(&client_msg.user) {
    trace!(
      "[💭Server]: receives client message: {}",
      client_msg.content,
    );
    match client_stream
      .stream_tx
      .send(Ok(RealtimeMessage::from(client_msg.clone())))
    {
      Ok(_) => {},
      Err(e) => {
        tracing::error!("send error: {}", e)
      },
    }
  }
}

/// Remove the user from the group and remove the group from the cache if the group is empty.
async fn remove_user_from_group<S, U>(
  user: &U,
  groups: &Arc<CollabGroupCache<S, U>>,
  editing: &Editing,
) where
  S: CollabStorage,
  U: RealtimeUser,
{
  if let Some(group) = groups.get_group(&editing.object_id).await {
    info!("Remove subscriber: {}", editing.origin);
    group.subscribers.write().await.remove(user);
    let should_remove = group.is_empty().await;
    if should_remove {
      group.save_collab();

      tracing::debug!("Remove group: {}", editing.object_id);
      groups.remove_group(&editing.object_id).await;
    }
  }
}

impl<S, U, P> actix::Supervised for CollabServer<S, U, P>
where
  S: 'static + Unpin,
  U: RealtimeUser + Unpin,
  P: CollabPermission + Unpin,
{
  fn restarting(&mut self, _ctx: &mut Context<CollabServer<S, U, P>>) {
    tracing::warn!("restarting");
  }
}

impl TryFrom<RealtimeMessage> for CollabMessage {
  type Error = StreamError;

  fn try_from(value: RealtimeMessage) -> Result<Self, Self::Error> {
    CollabMessage::from_vec(&value.payload).map_err(|e| StreamError::Internal(e.to_string()))
  }
}

pub struct CollabClientStream {
  ws_sink: ClientWSSink,
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
    Self {
      ws_sink: sink,
      stream_tx,
    }
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
    ReceiverStream<Result<T, StreamError>>,
  )
  where
    T:
      TryFrom<RealtimeMessage, Error = StreamError> + Into<RealtimeMessage> + Send + Sync + 'static,
    SinkFilter: Fn(&str, &T) -> BoxFuture<'static, bool> + Sync + Send + 'static,
    StreamFilter: Fn(&str, &RealtimeMessage) -> BoxFuture<'static, bool> + Sync + Send + 'static,
  {
    let client_ws_sink = self.ws_sink.clone();
    let mut stream_rx = BroadcastStream::new(self.stream_tx.subscribe());
    let cloned_object_id = object_id.to_string();

    // Send the message to the connected websocket client
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<T>();
    tokio::task::spawn(async move {
      while let Some(msg) = rx.recv().await {
        let can_sink = sink_filter(&cloned_object_id, &msg).await;
        if can_sink {
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
      while let Some(Ok(Ok(msg))) = stream_rx.next().await {
        if stream_filter(&cloned_object_id, &msg).await {
          let _ = tx.send(T::try_from(msg)).await;
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
}
