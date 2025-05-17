use super::db::Db;
use super::WorkspaceId;
use crate::entity::CollabType;
use crate::v2::actor::{WorkspaceAction, WorkspaceControllerActor, WsConn};
use appflowy_proto::{Rid, WorkspaceNotification};
use collab_plugins::local_storage::rocksdb::kv_impl::KVTransactionDBRocksdbImpl;
use collab_rt_protocol::CollabRef;
use futures_util::stream::SplitSink;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use yrs::block::ClientID;

#[derive(Clone)]
pub struct WorkspaceController {
  actor: Arc<WorkspaceControllerActor>,
}

impl WorkspaceController {
  pub fn new(options: Options, workspace_db_path: &str) -> anyhow::Result<Self> {
    let db = Db::open(options.workspace_id, options.uid, workspace_db_path)?;
    Self::new_with_db(options, db)
  }

  pub fn new_with_rocksdb(
    options: Options,
    db: KVTransactionDBRocksdbImpl,
  ) -> anyhow::Result<Self> {
    let db = Db::open_with_rocksdb(options.workspace_id, options.uid, db)?;
    Self::new_with_db(options, db)
  }

  fn new_with_db(options: Options, db: Db) -> anyhow::Result<Self> {
    let last_message_id = db.last_message_id()?;
    let actor = WorkspaceControllerActor::new(db, options, last_message_id);
    Ok(Self { actor })
  }

  pub fn is_connected(&self) -> bool {
    matches!(
      &*self.actor.status_channel().borrow(),
      ConnectionStatus::Connected { .. }
    )
  }

  pub fn is_disconnected(&self) -> bool {
    matches!(
      &*self.actor.status_channel().borrow(),
      ConnectionStatus::Disconnected { .. }
    )
  }

  pub fn connect_state(&self) -> ConnectState {
    ConnectState::from(&*self.actor.status_channel().borrow())
  }

  pub fn subscribe_notification(&self) -> tokio::sync::broadcast::Receiver<WorkspaceNotification> {
    self.actor.subscribe_notification()
  }

  pub async fn connect(&self, access_token: String) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    self.actor.trigger(WorkspaceAction::Connect {
      ack: tx,
      access_token,
    });
    rx.await??;
    Ok(())
  }

  pub async fn disconnect(&self) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    self.actor.trigger(WorkspaceAction::Disconnect(tx));
    rx.await??;
    Ok(())
  }

  pub async fn close(&mut self) -> anyhow::Result<()> {
    self.disconnect().await
  }

  pub fn client_id(&self) -> ClientID {
    self.actor.client_id()
  }

  pub fn workspace_id(&self) -> WorkspaceId {
    *self.actor.workspace_id()
  }

  pub fn last_message_id(&self) -> Rid {
    self.actor.last_message_id()
  }

  pub async fn bind(&self, collab_ref: &CollabRef, collab_type: CollabType) -> anyhow::Result<()> {
    WorkspaceControllerActor::bind(&self.actor, collab_ref, collab_type).await
  }

  /// Binds a collaboration object to the actor and loads its data if needed.
  /// This function sets up the necessary callbacks and observers to handle
  /// collaboration updates and awareness changes.
  ///
  /// # Arguments
  ///
  /// * `actor`: Reference to the workspace controller actor managing the collaboration
  /// * `collab_ref`: Reference to the collaboration object to be bound
  /// * `collab_type`: The type of the collaboration (document, folder, etc.)
  /// * `init_collab`: Whether to initialize the collaboration data or not
  ///   - When true, attempts to initialize the collab from local disk
  ///   - Returns false from db.init_collab() if the document already exists in the database
  ///   - In this case, the existing document will be loaded from the local disk
  pub async fn bind_and_init_collab(
    &self,
    collab_ref: &CollabRef,
    collab_type: CollabType,
    init_collab: bool,
  ) -> anyhow::Result<()> {
    WorkspaceControllerActor::bind_and_init_collab(
      &self.actor,
      collab_ref,
      collab_type,
      init_collab,
    )
    .await
  }
}

#[cfg(debug_assertions)]
impl WorkspaceController {
  pub fn enable_receive_message(&self) {
    self
      .actor
      .skip_realtime_message
      .store(false, std::sync::atomic::Ordering::SeqCst);
  }

  pub fn disable_receive_message(&self) {
    self
      .actor
      .skip_realtime_message
      .store(true, std::sync::atomic::Ordering::SeqCst);
  }
}

#[derive(Debug, Clone)]
pub enum ConnectionStatus {
  Disconnected {
    reason: Option<Arc<str>>,
    retry: bool,
  },
  Connecting {
    cancel: CancellationToken,
  },
  Connected {
    sink: Arc<Mutex<SplitSink<WsConn, Message>>>,
    cancel: CancellationToken,
  },
}

impl Default for ConnectionStatus {
  fn default() -> Self {
    ConnectionStatus::Disconnected {
      reason: None,
      retry: false,
    }
  }
}

impl Display for ConnectionStatus {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      ConnectionStatus::Disconnected {
        reason: None,
        retry: _,
      } => write!(f, "disconnected"),
      ConnectionStatus::Disconnected {
        reason: Some(reason),
        retry,
      } => write!(f, "disconnected: {}, retry: {}", reason, retry),
      ConnectionStatus::Connecting { .. } => write!(f, "connecting"),
      ConnectionStatus::Connected { .. } => write!(f, "connected"),
    }
  }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ConnectState {
  Disconnected {
    reason: Option<Arc<str>>,
    retry: bool,
  },
  Connecting,
  Connected,
}

impl From<&ConnectionStatus> for ConnectState {
  fn from(value: &ConnectionStatus) -> Self {
    match value {
      ConnectionStatus::Disconnected { reason, retry } => ConnectState::Disconnected {
        reason: reason.clone(),
        retry: *retry,
      },
      ConnectionStatus::Connecting { .. } => ConnectState::Connecting,
      ConnectionStatus::Connected { .. } => ConnectState::Connected,
    }
  }
}

impl ConnectState {
  pub fn is_connected(&self) -> bool {
    matches!(self, ConnectState::Connected)
  }
}

#[derive(Debug, Clone)]
pub struct Options {
  /// Endpoint where server V2 protocol handler is listening on
  /// (i.e. `ws://{server}:8000/ws/v2`).
  pub url: String,
  /// UUID of a workspace this controller is responsible for.
  pub workspace_id: WorkspaceId,
  /// Unique user ID assigned by the server.
  pub uid: i64,
  /// Unique identifier of current device
  pub device_id: String,
  /// If true, when connected, it will try to fetch info about new collabs
  /// created while this client was offline.
  pub sync_eagerly: bool,
}
