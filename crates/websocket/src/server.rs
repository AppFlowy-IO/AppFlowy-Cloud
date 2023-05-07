use crate::entities::{ClientMessage, Connect, Disconnect, ServerMessage, WSUser};
use crate::error::WSError;
use crate::ClientSink;
use actix::dev::channel::channel;
use actix::{Actor, AsyncContext, Context, Handler, Recipient, ResponseFuture};
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab_persistence::kv::rocks_kv::RocksCollabDB;
use collab_sync::server::{CollabBroadcast, CollabGroup};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::{Sender, UnboundedReceiver};
use tokio_stream::wrappers::ReceiverStream;

pub type CollabId = String;

#[derive(Clone)]
pub struct CollabServer {
  db: Arc<RocksCollabDB>,
  collab_groups: Arc<RwLock<HashMap<CollabId, CollabGroup>>>,
  connect_counter: Arc<AtomicUsize>,
  client_conns: Arc<RwLock<HashMap<Arc<WSUser>, ClientStream>>>,
}

impl CollabServer {
  pub fn new(db: Arc<RocksCollabDB>) -> Result<Self, WSError> {
    let connect_counter = AtomicUsize::new(0);
    Ok(Self {
      db,
      collab_groups: Default::default(),
      connect_counter: Arc::new(connect_counter),
      client_conns: Default::default(),
    })
  }

  fn next_connection_id(&self) -> usize {
    self.connect_counter.fetch_add(1, Ordering::SeqCst)
  }
}

impl Actor for CollabServer {
  type Context = Context<Self>;
}

impl Handler<Connect> for CollabServer {
  type Result = Result<(), WSError>;
  // type Result = ResponseFuture<Result<(), WSError>>;

  fn handle(&mut self, msg: Connect, ctx: &mut Context<Self>) -> Self::Result {
    let (stream_tx, rx) = tokio::sync::mpsc::channel::<ClientMessage>(100);
    let stream = ClientStream::new(ClientSink(msg.socket), ReceiverStream::new(rx), stream_tx);
    self.client_conns.write().insert(msg.user, stream);
    Ok(())
  }
}

impl Handler<Disconnect> for CollabServer {
  type Result = Result<(), WSError>;
  fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) -> Self::Result {
    Ok(())
  }
}

impl Handler<ClientMessage> for CollabServer {
  type Result = ();

  fn handle(&mut self, msg: ClientMessage, _ctx: &mut Context<Self>) -> Self::Result {
    self
      .client_conns
      .read()
      .get(&msg.user)
      .map(|client_stream| {
        client_stream.stream_tx.send(msg);
      });
  }
}

impl actix::Supervised for CollabServer {
  fn restarting(&mut self, _ctx: &mut Context<CollabServer>) {
    tracing::warn!("restarting");
  }
}

pub struct ClientStream {
  sink: Option<ClientSink>,
  stream: Option<ReceiverStream<ClientMessage>>,
  stream_tx: Sender<ClientMessage>,
}

impl ClientStream {
  pub fn new(
    sink: ClientSink,
    stream: ReceiverStream<ClientMessage>,
    stream_tx: Sender<ClientMessage>,
  ) -> Self {
    Self {
      sink: Some(sink),
      stream: Some(stream),
      stream_tx,
    }
  }
}
