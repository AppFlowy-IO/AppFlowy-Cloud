use crate::entities::{ClientMessage, Connect, Disconnect, WSUser};
use crate::error::WSError;
use crate::ClientSink;

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
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::Sender;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Clone)]
pub struct CollabServer {
  db: Arc<RocksCollabDB>,
  /// Generate collab_id for new collab object
  collab_id_gen: Arc<Mutex<CollabIDGen>>,
  /// Memory cache for fast lookup of collab_id from object_id
  collab_id_by_object_id: Arc<DashMap<String, CollabId>>,
  collab_groups: Arc<RwLock<HashMap<CollabId, CollabGroup>>>,
  client_streams: Arc<RwLock<HashMap<Arc<WSUser>, ClientStream>>>,
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

  fn create_collab_id(&self, object_id: &str) -> Result<CollabId, WSError> {
    let collab_id = self.collab_id_gen.lock().next_id();
    let collab_key = make_collab_id_key(object_id.as_ref());
    self.db.with_write_txn(|w_txn| {
      w_txn.insert(collab_key.as_ref(), collab_id.to_be_bytes())?;
      Ok(())
    })?;
    Ok(collab_id)
  }

  fn get_collab_id(&self, object_id: &str) -> Option<CollabId> {
    let collab_key = make_collab_id_key(object_id.as_ref());
    let read_txn = self.db.read_txn();
    let value = read_txn.get(collab_key.as_ref()).ok()??;

    let mut bytes = [0; COLLAB_ID_LEN];
    bytes[0..COLLAB_ID_LEN].copy_from_slice(value.as_ref());
    Some(CollabId::from_be_bytes(bytes))
  }

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

  fn create_group_if_need(&self, collab_id: CollabId, object_id: &str) {
    if self.collab_groups.read().contains_key(&collab_id) {
      return;
    }

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
    let (stream_tx, rx) = tokio::sync::mpsc::channel(100);
    let stream = ClientStream::new(
      ClientSink(new_conn.socket),
      ReceiverStream::new(rx),
      stream_tx,
    );
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
    if let Ok(collab_id) = self.get_or_create_collab_id(object_id) {
      if let Some(collab_group) = self.collab_groups.write().get_mut(&collab_id) {
        if let Some(client_stream) = self.client_streams.write().get_mut(&client_msg.user) {
          if let Some((sink, stream)) = client_stream.split() {
            let origin = match client_msg.collab_msg.origin() {
              None => CollabOrigin::Empty,
              Some(client) => client.clone(),
            };
            let sub = collab_group
              .broadcast
              .subscribe(origin.clone(), sink, stream);
            collab_group.subscribers.insert(origin, sub);
          }
        }
      }

      let client_streams = self.client_streams.clone();
      Box::pin(async move {
        if let Some(client_stream) = client_streams.read().get(&client_msg.user) {
          tracing::trace!(
            "[WSServer]: receives client message: {:?}",
            client_msg.collab_msg.msg_id()
          );
          match client_stream
            .stream_tx
            .send(Ok(client_msg.collab_msg))
            .await
          {
            Ok(_) => {},
            Err(e) => tracing::trace!("send error: {:?}", e),
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

pub struct ClientStream {
  sink: Option<ClientSink>,
  stream: Option<ReceiverStream<Result<CollabMessage, WSError>>>,
  stream_tx: Sender<Result<CollabMessage, WSError>>,
}

impl ClientStream {
  pub fn new(
    sink: ClientSink,
    stream: ReceiverStream<Result<CollabMessage, WSError>>,
    stream_tx: Sender<Result<CollabMessage, WSError>>,
  ) -> Self {
    Self {
      sink: Some(sink),
      stream: Some(stream),
      stream_tx,
    }
  }

  pub fn split(&mut self) -> Option<(ClientSink, ReceiverStream<Result<CollabMessage, WSError>>)> {
    let sink = self.sink.take()?;
    let stream = self.stream.take()?;
    Some((sink, stream))
  }
}
