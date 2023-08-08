use crate::entities::{ClientMessage, Connect, Disconnect, ServerMessage, WSMessage, WSUser};
use crate::error::WSError;
use crate::ClientSink;

use crate::channel_ext::UnboundedSenderSink;
use actix::{Actor, Context, Handler, ResponseFuture};
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab_plugins::disk::keys::make_collab_id_key;
use collab_plugins::disk::kv::rocks_kv::RocksCollabDB;
use collab_plugins::disk::kv::KVStore;
use collab_plugins::disk::rocksdb_server::RocksdbServerDiskPlugin;
use collab_plugins::sync::msg::CollabMessage;
use collab_plugins::sync::server::{
  CollabBroadcast, CollabGroup, CollabIDGen, CollabId, NonZeroNodeId, COLLAB_ID_LEN,
};
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;

use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_stream::StreamExt;

#[derive(Clone)]
pub struct CollabServer {
  db: Arc<RocksCollabDB>,
  /// Generate collab_id for new collab object
  collab_id_gen: Arc<Mutex<CollabIDGen>>,
  /// Memory cache for fast lookup of collab_id from object_id
  collab_id_by_object_id: Arc<DashMap<String, CollabId>>,
  /// Keep track of all collab groups
  collab_groups: Arc<RwLock<HashMap<CollabId, CollabGroup>>>,
  /// Keep track of all client streams
  client_streams: Arc<RwLock<HashMap<Arc<WSUser>, WSClientStream>>>,
}

impl CollabServer {
  pub fn new(db: Arc<RocksCollabDB>) -> Result<Self, WSError> {
    let collab_id_gen = Arc::new(Mutex::new(CollabIDGen::new(NonZeroNodeId(1))));
    let collab_id_by_object_id = Arc::new(DashMap::new());
    Ok(Self {
      db,
      collab_id_gen,
      collab_id_by_object_id,
      collab_groups: Default::default(),
      client_streams: Default::default(),
    })
  }

  /// Create a new collab id for the object id.
  fn create_collab_id(&self, object_id: &str) -> Result<CollabId, WSError> {
    let collab_id = self.collab_id_gen.lock().next_id();
    let collab_key = make_collab_id_key(object_id.as_ref());
    self
      .db
      .with_write_txn(|w_txn| {
        w_txn.insert(collab_key.as_ref(), collab_id.to_be_bytes())?;
        Ok(())
      })
      .map_err(|e| WSError::Internal(e.to_string()))?;
    tracing::trace!("[WSServer]: Create new collab id: {}", collab_id);
    Ok(collab_id)
  }

  /// Get the collab id for the object
  /// If the object doesn't have a collab id, return None
  fn get_collab_id(&self, object_id: &str) -> Option<CollabId> {
    let collab_key = make_collab_id_key(object_id.as_ref());
    let read_txn = self.db.read_txn();
    let value = read_txn.get(collab_key.as_ref()).ok()??;

    let mut bytes = [0; COLLAB_ID_LEN];
    bytes[0..COLLAB_ID_LEN].copy_from_slice(value.as_ref());
    Some(CollabId::from_be_bytes(bytes))
  }

  /// Get or create a collab id if the object doesn't have one
  fn get_or_create_collab_id(&self, object_id: &str) -> Result<CollabId, WSError> {
    let collab_id = self.get_collab_id(object_id);
    if let Some(collab_id) = collab_id {
      self.create_group_if_need(collab_id, object_id);
      Ok(collab_id)
    } else {
      let collab_id = self.create_collab_id(object_id)?;
      self
        .collab_id_by_object_id
        .insert(object_id.to_string(), collab_id);
      self.create_group_if_need(collab_id, object_id);
      Ok(collab_id)
    }
  }

  /// Create the collab group for the object if it doesn't exist
  fn create_group_if_need(&self, collab_id: CollabId, object_id: &str) {
    if self.collab_groups.read().contains_key(&collab_id) {
      return;
    }
    tracing::trace!(
      "[WSServer]: Create new group: collab_id:{} object_id:{}",
      collab_id,
      object_id
    );

    let collab = MutexCollab::new(CollabOrigin::Empty, object_id, vec![]);
    let plugin = RocksdbServerDiskPlugin::new(collab_id, self.db.clone()).unwrap();
    collab.lock().add_plugin(Arc::new(plugin));
    collab.initial();

    let broadcast = CollabBroadcast::new(object_id, collab.clone(), 10);
    let group = CollabGroup {
      collab,
      broadcast,
      subscribers: Default::default(),
    };
    self.collab_groups.write().insert(collab_id, group);
  }
}

impl Actor for CollabServer {
  type Context = Context<Self>;
}

impl Handler<Connect> for CollabServer {
  type Result = Result<(), WSError>;

  fn handle(&mut self, new_conn: Connect, _ctx: &mut Context<Self>) -> Self::Result {
    tracing::trace!("[WSServer]: {} connect", new_conn.user);

    let stream = WSClientStream::new(ClientSink(new_conn.socket));
    self.client_streams.write().insert(new_conn.user, stream);
    Ok(())
  }
}

impl Handler<Disconnect> for CollabServer {
  type Result = Result<(), WSError>;
  fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) -> Self::Result {
    tracing::trace!("[WSServer]: {} disconnect", msg.user);
    self.client_streams.write().remove(&msg.user);
    Ok(())
  }
}

impl Handler<ClientMessage> for CollabServer {
  type Result = ResponseFuture<()>;

  fn handle(&mut self, client_msg: ClientMessage, _ctx: &mut Context<Self>) -> Self::Result {
    let object_id = client_msg.collab_msg.object_id();
    // Get the collab_id for the object_id. If the object_id is not exist, create a new collab_id for it.
    // Also create a new [CollabGroup] for the collab_id if it is not exist.
    if let Ok(collab_id) = self.get_or_create_collab_id(object_id) {
      if let Some(collab_group) = self.collab_groups.write().get_mut(&collab_id) {
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
          if let Some(client_stream) = self.client_streams.write().get_mut(&client_msg.user) {
            if let Some((sink, stream)) = client_stream.stream_object::<CollabMessage, _, _>(
              object_id.to_string(),
              move |object_id, msg| msg.object_id() == object_id,
              move |object_id, msg| msg.object_id == object_id,
            ) {
              tracing::trace!(
                "[WSServer]: {} subscribe group:{}",
                client_msg.user,
                collab_id
              );
              let subscription = collab_group
                .broadcast
                .subscribe(origin.clone(), sink, stream);
              collab_group.subscribers.insert(origin, subscription);
            }
          }
        }
      }

      let client_streams = self.client_streams.clone();
      Box::pin(async move {
        if let Some(client_stream) = client_streams.read().get(&client_msg.user) {
          tracing::trace!(
            "[WSServer]: receives: [collab_id:{}|oid:{}|msg_id:{:?}]",
            collab_id,
            client_msg.collab_msg.object_id(),
            client_msg.collab_msg.msg_id()
          );
          match client_stream
            .stream_tx
            .send(Ok(WSMessage::from(client_msg)))
          {
            Ok(_) => {},
            Err(e) => tracing::error!("🔴send error: {:?}", e),
          }
        }
      })
    } else {
      Box::pin(async move {})
    }
  }
}

impl actix::Supervised for CollabServer {
  fn restarting(&mut self, _ctx: &mut Context<CollabServer>) {
    tracing::warn!("restarting");
  }
}

pub struct WSClientStream {
  sink: ClientSink,
  stream_tx: tokio::sync::broadcast::Sender<Result<WSMessage, WSError>>,
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
  ) -> Option<(UnboundedSenderSink<T>, ReceiverStream<Result<T, WSError>>)>
  where
    T: TryFrom<WSMessage, Error = WSError> + Into<ServerMessage> + Send + Sync + 'static,
    F1: Fn(&str, &T) -> bool + Send + Sync + 'static,
    F2: Fn(&str, &WSMessage) -> bool + Send + Sync + 'static,
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

impl TryFrom<WSMessage> for CollabMessage {
  type Error = WSError;

  fn try_from(value: WSMessage) -> Result<Self, Self::Error> {
    CollabMessage::from_vec(&value.payload).map_err(|e| WSError::Internal(e.to_string()))
  }
}
