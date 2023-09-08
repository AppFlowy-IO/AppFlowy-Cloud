use crate::entities::{
  ClientMessage, Connect, Disconnect, EditCollab, RealtimeMessage, RealtimeUser, ServerMessage,
};
use crate::error::{RealtimeError, StreamError};
use anyhow::Result;

use crate::util::channel_ext::UnboundedSenderSink;
use actix::{Actor, Context, Handler, ResponseFuture};
use collab::core::origin::CollabOrigin;

use collab_sync_protocol::CollabMessage;
use parking_lot::RwLock;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio_stream::StreamExt;

use crate::core::group::CollabGroupCache;
use crate::core::ClientWebsocketSink;
use storage::collab::CollabStorage;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

#[derive(Clone)]
pub struct CollabServer {
  #[allow(dead_code)]
  storage: Arc<CollabStorage>,
  /// Keep track of all collab groups
  groups: Arc<CollabGroupCache>,
  /// Keep track of all object ids that a user is subscribed to
  edit_collab_by_user: Arc<RwLock<HashMap<Arc<RealtimeUser>, HashSet<EditCollab>>>>,
  /// Keep track of all client streams
  client_streams: Arc<RwLock<HashMap<Arc<RealtimeUser>, RealtimeClientStream>>>,
}

impl CollabServer {
  pub fn new(storage: Arc<CollabStorage>) -> Result<Self, RealtimeError> {
    let groups = Arc::new(CollabGroupCache::new(storage.clone()));
    let edit_collab_by_user = Arc::new(RwLock::new(HashMap::new()));
    Ok(Self {
      storage,
      groups,
      edit_collab_by_user,
      client_streams: Default::default(),
    })
  }
}

impl Actor for CollabServer {
  type Context = Context<Self>;
}

impl Handler<Connect> for CollabServer {
  type Result = Result<(), RealtimeError>;

  fn handle(&mut self, new_conn: Connect, _ctx: &mut Context<Self>) -> Self::Result {
    tracing::trace!("[CollabServer]: {} connect", new_conn.user);

    let stream = RealtimeClientStream::new(ClientWebsocketSink(new_conn.socket));
    self.client_streams.write().insert(new_conn.user, stream);

    Ok(())
  }
}

impl Handler<Disconnect> for CollabServer {
  type Result = Result<(), RealtimeError>;
  fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) -> Self::Result {
    tracing::trace!("[CollabServer]: {} disconnect", msg.user);
    self.client_streams.write().remove(&msg.user);

    // Remove the user from all collab groups that the user is subscribed to
    let edits = self.edit_collab_by_user.write().remove(&msg.user);
    if let Some(edits) = edits {
      if !edits.is_empty() {
        let groups = self.groups.clone();
        tokio::task::spawn_blocking(move || {
          for edit in edits {
            remove_user_from_group(&groups, &edit);
          }
        });
      }
    }

    Ok(())
  }
}

impl Handler<ClientMessage> for CollabServer {
  type Result = ResponseFuture<Result<(), RealtimeError>>;

  fn handle(&mut self, client_msg: ClientMessage, _ctx: &mut Context<Self>) -> Self::Result {
    let client_streams = self.client_streams.clone();
    let groups = self.groups.clone();
    let edit_collab_by_user = self.edit_collab_by_user.clone();

    Box::pin(async move {
      subscribe_collab_group_change_if_need(
        &client_msg,
        &groups,
        &edit_collab_by_user,
        &client_streams,
      )
      .await?;
      forward_message_to_collab_group(&client_msg, &client_streams).await;
      Ok(())
    })
  }
}

async fn forward_message_to_collab_group(
  client_msg: &ClientMessage,
  client_streams: &Arc<RwLock<HashMap<Arc<RealtimeUser>, RealtimeClientStream>>>,
) {
  if let Some(client_stream) = client_streams.read().get(&client_msg.user) {
    tracing::trace!(
      "[CollabServer]: receives: [oid:{}|msg_id:{:?}]",
      client_msg.content.object_id(),
      client_msg.content.msg_id()
    );
    match client_stream
      .stream_tx
      .send(Ok(RealtimeMessage::from(client_msg.clone())))
    {
      Ok(_) => {},
      Err(e) => tracing::error!("🔴send error: {:?}", e),
    }
  }
}

async fn subscribe_collab_group_change_if_need(
  client_msg: &ClientMessage,
  groups: &Arc<CollabGroupCache>,
  edit_collab_by_user: &Arc<RwLock<HashMap<Arc<RealtimeUser>, HashSet<EditCollab>>>>,
  client_streams: &Arc<RwLock<HashMap<Arc<RealtimeUser>, RealtimeClientStream>>>,
) -> Result<(), RealtimeError> {
  let object_id = client_msg.content.object_id();
  if !groups.read().contains_key(object_id) {
    // When create a group, the message must be the init sync message.
    match &client_msg.content {
      CollabMessage::ClientInit(client_init) => {
        groups
          .create_group(&client_init.workspace_id, object_id)
          .await;
      },
      _ => {
        return Err(RealtimeError::UnexpectedData(
          "The first message must be init sync message",
        ));
      },
    }
  }

  let origin = match client_msg.content.origin() {
    None => {
      tracing::error!("🔴The origin from client message is empty");
      &CollabOrigin::Empty
    },
    Some(client) => client,
  };

  // If the client's stream is already subscribed to the collab group, return.
  if groups
    .read()
    .get(object_id)
    .map(|group| group.subscribers.get(origin))
    .is_some()
  {
    return Ok(());
  }

  match client_streams.write().get_mut(&client_msg.user) {
    None => tracing::error!("🔴The client stream is not found"),
    Some(client_stream) => {
      if let Some(collab_group) = groups.write().get_mut(object_id) {
        collab_group
          .subscribers
          .entry(origin.clone())
          .or_insert_with(|| {
            tracing::trace!(
              "[CollabServer]: {} subscribe group:{}",
              client_msg.user,
              client_msg.content.object_id()
            );

            edit_collab_by_user
              .write()
              .entry(client_msg.user.clone())
              .or_default()
              .insert(EditCollab {
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
}

fn remove_user_from_group(groups: &Arc<CollabGroupCache>, edit_collab: &EditCollab) {
  let mut write_guard = groups.write();

  let should_remove_group = write_guard.get_mut(&edit_collab.object_id).map(|group| {
    group.subscribers.remove(&edit_collab.origin);
    let should_remove = group.subscribers.is_empty();
    if should_remove {
      group.flush_collab();
    }
    should_remove
  });

  // If the group is empty, remove it from the cache
  if should_remove_group.unwrap_or(false) {
    write_guard.remove(&edit_collab.object_id);
  }
}

impl actix::Supervised for CollabServer {
  fn restarting(&mut self, _ctx: &mut Context<CollabServer>) {
    tracing::warn!("restarting");
  }
}

pub struct RealtimeClientStream {
  ws_sink: ClientWebsocketSink,
  /// Used to receive messages from the collab server
  stream_tx: tokio::sync::broadcast::Sender<Result<RealtimeMessage, StreamError>>,
}

impl RealtimeClientStream {
  pub fn new(sink: ClientWebsocketSink) -> Self {
    // When receive a new connection, create a new [ClientStream] that holds the connection's websocket
    let (stream_tx, _) = tokio::sync::broadcast::channel(1000);
    Self {
      ws_sink: sink,
      stream_tx,
    }
  }

  /// Returns a [UnboundedSenderSink] and a [ReceiverStream] for the object_id.
  #[allow(clippy::type_complexity)]
  pub fn client_channel<T, F1, F2>(
    &mut self,
    object_id: &str,
    sink_filter: F1,
    stream_filter: F2,
  ) -> Option<(
    UnboundedSenderSink<T>,
    ReceiverStream<Result<T, StreamError>>,
  )>
  where
    T: TryFrom<RealtimeMessage, Error = StreamError> + Into<ServerMessage> + Send + Sync + 'static,
    F1: Fn(&str, &T) -> bool + Send + Sync + 'static,
    F2: Fn(&str, &RealtimeMessage) -> bool + Send + Sync + 'static,
  {
    let client_ws_sink = self.ws_sink.clone();
    let mut stream_rx = BroadcastStream::new(self.stream_tx.subscribe());
    let cloned_object_id = object_id.to_string();

    // Send the message to the connected websocket client
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<T>();
    tokio::spawn(async move {
      while let Some(msg) = rx.recv().await {
        if sink_filter(&cloned_object_id, &msg) {
          client_ws_sink.do_send(msg.into());
        }
      }
    });
    let client_forward_sink = UnboundedSenderSink::<T>::new(tx);

    // forward the message to the stream that can be subscribed by the broadcast group, which will
    // send the messages to all connected clients using the client_forward_sink
    let cloned_object_id = object_id.to_string();
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    tokio::spawn(async move {
      while let Some(Ok(Ok(msg))) = stream_rx.next().await {
        if stream_filter(&cloned_object_id, &msg) {
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
    Some((client_forward_sink, client_forward_stream))
  }
}

impl TryFrom<RealtimeMessage> for CollabMessage {
  type Error = StreamError;

  fn try_from(value: RealtimeMessage) -> Result<Self, Self::Error> {
    CollabMessage::from_vec(&value.payload).map_err(|e| StreamError::Internal(e.to_string()))
  }
}
