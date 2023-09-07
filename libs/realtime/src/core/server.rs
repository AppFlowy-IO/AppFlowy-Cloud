use crate::entities::{
  ClientMessage, Connect, Disconnect, RealtimeMessage, RealtimeUser, ServerMessage,
};
use crate::error::{CollabSyncError, StreamError};
use anyhow::Result;

use crate::util::channel_ext::UnboundedSenderSink;
use actix::{Actor, Context, Handler, ResponseFuture};
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;

use collab_sync_protocol::CollabMessage;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio_stream::StreamExt;

use crate::collab::{CollabBroadcast, CollabGroup, CollabStoragePlugin};
use crate::core::ClientSink;
use storage::collab::CollabStorage;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

#[derive(Clone)]
pub struct CollabServer {
  #[allow(dead_code)]
  storage: Arc<CollabStorage>,
  /// Keep track of all collab groups
  group_cache: Arc<CollabGroupCache>,
  /// Keep track of all client streams
  client_streams: Arc<RwLock<HashMap<Arc<RealtimeUser>, WSClientStream>>>,
}

impl CollabServer {
  pub fn new(storage: Arc<CollabStorage>) -> Result<Self, CollabSyncError> {
    let group_cache = Arc::new(CollabGroupCache::new(storage.clone()));
    Ok(Self {
      storage,
      group_cache,
      client_streams: Default::default(),
    })
  }
}

impl Actor for CollabServer {
  type Context = Context<Self>;
}

impl Handler<Connect> for CollabServer {
  type Result = Result<(), CollabSyncError>;

  fn handle(&mut self, new_conn: Connect, _ctx: &mut Context<Self>) -> Self::Result {
    tracing::trace!("[CollabServer]: {} connect", new_conn.user);

    let stream = WSClientStream::new(ClientSink(new_conn.socket));
    self.client_streams.write().insert(new_conn.user, stream);
    Ok(())
  }
}

impl Handler<Disconnect> for CollabServer {
  type Result = Result<(), CollabSyncError>;
  fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) -> Self::Result {
    tracing::trace!("[CollabServer]: {} disconnect", msg.user);
    self.client_streams.write().remove(&msg.user);
    Ok(())
  }
}

impl Handler<ClientMessage> for CollabServer {
  type Result = ResponseFuture<()>;

  fn handle(&mut self, client_msg: ClientMessage, _ctx: &mut Context<Self>) -> Self::Result {
    // Get the collab_id for the object_id. If the object_id is not exist, create a new collab_id for it.
    // Also create a new [CollabGroup] for the collab_id if it is not exist.

    let client_streams = self.client_streams.clone();
    let groups = self.group_cache.clone();
    Box::pin(async move {
      let object_id = client_msg.collab_msg.object_id();

      if !groups.read().contains_key(object_id) {
        groups.create_group(object_id).await;
      }

      if let Some(collab_group) = groups.write().get_mut(object_id) {
        let origin = match client_msg.collab_msg.origin() {
          None => {
            tracing::error!("🔴The origin from client message is empty");
            CollabOrigin::Empty
          },
          Some(client) => client.clone(),
        };

        let is_subscribe = collab_group.subscribers.get(&origin).is_some();
        // If the client's stream is not subscribed to the collab group, subscribe it.
        if !is_subscribe {
          if let Some(client_stream) = client_streams.write().get_mut(&client_msg.user) {
            if let Some((sink, stream)) = client_stream.stream_object::<CollabMessage, _, _>(
              object_id.to_string(),
              move |object_id, msg| msg.object_id() == object_id,
              move |object_id, msg| msg.object_id == object_id,
            ) {
              tracing::trace!(
                "[CollabServer]: {} subscribe group:{}",
                client_msg.user,
                client_msg.collab_msg.object_id()
              );
              let subscription = collab_group
                .broadcast
                .subscribe(origin.clone(), sink, stream);
              collab_group.subscribers.insert(origin, subscription);
            }
          }
        }
      }

      if let Some(client_stream) = client_streams.read().get(&client_msg.user) {
        tracing::trace!(
          "[CollabServer]: receives: [oid:{}|msg_id:{:?}]",
          client_msg.collab_msg.object_id(),
          client_msg.collab_msg.msg_id()
        );
        match client_stream
          .stream_tx
          .send(Ok(RealtimeMessage::from(client_msg)))
        {
          Ok(_) => {},
          Err(e) => tracing::error!("🔴send error: {:?}", e),
        }
      }
    })
  }
}

impl actix::Supervised for CollabServer {
  fn restarting(&mut self, _ctx: &mut Context<CollabServer>) {
    tracing::warn!("restarting");
  }
}

pub struct WSClientStream {
  sink: ClientSink,
  stream_tx: tokio::sync::broadcast::Sender<Result<RealtimeMessage, StreamError>>,
}

impl WSClientStream {
  pub fn new(sink: ClientSink) -> Self {
    // When receive a new connection, create a new [ClientStream] that holds the connection's websocket
    let (stream_tx, _) = tokio::sync::broadcast::channel(1000);
    Self { sink, stream_tx }
  }

  /// Returns a [UnboundedSenderSink] and a [ReceiverStream] for the object_id.
  #[allow(clippy::type_complexity)]
  pub fn stream_object<T, F1, F2>(
    &mut self,
    object_id: String,
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
    let client_sink = self.sink.clone();
    let mut stream = BroadcastStream::new(self.stream_tx.subscribe());
    let cloned_object_id = object_id.clone();

    // forward sink
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<T>();
    tokio::spawn(async move {
      while let Some(msg) = rx.recv().await {
        if sink_filter(&cloned_object_id, &msg) {
          client_sink.do_send(msg.into());
        }
      }
    });
    let sink = UnboundedSenderSink::<T>::new(tx);

    // forward stream
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    tokio::spawn(async move {
      while let Some(Ok(Ok(msg))) = stream.next().await {
        if stream_filter(&object_id, &msg) {
          let _ = tx.send(T::try_from(msg)).await;
        }
      }
    });
    let stream = ReceiverStream::new(rx);

    Some((sink, stream))
  }
}

struct CollabGroupCache {
  collab_group_by_object_id: RwLock<HashMap<String, CollabGroup>>,
  storage: Arc<CollabStorage>,
}

impl CollabGroupCache {
  fn new(storage: Arc<CollabStorage>) -> Self {
    Self {
      collab_group_by_object_id: RwLock::new(HashMap::new()),
      storage,
    }
  }

  async fn create_group(&self, object_id: &str) {
    if self
      .collab_group_by_object_id
      .read()
      .contains_key(object_id)
    {
      return;
    }

    let group = self.init_group(object_id).await;
    self
      .collab_group_by_object_id
      .write()
      .insert(object_id.to_string(), group);
  }

  async fn init_group(&self, object_id: &str) -> CollabGroup {
    tracing::trace!("Create new group for object_id:{}", object_id);

    let collab = MutexCollab::new(CollabOrigin::Empty, object_id, vec![]);
    let plugin = CollabStoragePlugin::new(self.storage.clone()).unwrap();
    collab.lock().add_plugin(Arc::new(plugin));
    collab.async_initialize().await;

    let broadcast = CollabBroadcast::new(object_id, collab.clone(), 10);
    CollabGroup {
      collab,
      broadcast,
      subscribers: Default::default(),
    }
  }
}

impl Deref for CollabGroupCache {
  type Target = RwLock<HashMap<String, CollabGroup>>;

  fn deref(&self) -> &Self::Target {
    &self.collab_group_by_object_id
  }
}

impl DerefMut for CollabGroupCache {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.collab_group_by_object_id
  }
}

impl TryFrom<RealtimeMessage> for CollabMessage {
  type Error = StreamError;

  fn try_from(value: RealtimeMessage) -> Result<Self, Self::Error> {
    CollabMessage::from_vec(&value.payload).map_err(|e| StreamError::Internal(e.to_string()))
  }
}
