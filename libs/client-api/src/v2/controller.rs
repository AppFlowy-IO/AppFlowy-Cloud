use super::db::Db;
use super::WorkspaceId;
use crate::entity::CollabType;
use crate::v2::actor::{WorkspaceAction, WorkspaceControllerActor, WsConn};
use appflowy_proto::Rid;
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
  pub fn new(options: Options) -> anyhow::Result<Self> {
    let db = Db::open(
      options.workspace_id,
      options.uid,
      &options.workspace_db_path,
    )?;
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

  pub async fn connect(&self) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    self.actor.trigger(WorkspaceAction::Connect(tx));
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
    ConnectionStatus::Disconnected { reason: None }
  }
}

impl Display for ConnectionStatus {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      ConnectionStatus::Disconnected { reason: None } => write!(f, "disconnected"),
      ConnectionStatus::Disconnected {
        reason: Some(reason),
      } => write!(f, "disconnected: {}", reason),
      ConnectionStatus::Connecting { .. } => write!(f, "connecting"),
      ConnectionStatus::Connected { .. } => write!(f, "connected"),
    }
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
  /// A local machine path, where current workspace related data should be stored.
  pub workspace_db_path: String,
  /// Unique identifier of current device
  pub device_id: String,
  /// Access token used for current client authentication.
  pub access_token: String,
  /// If true, when connected, it will try to fetch info about new collabs
  /// created while this client was offline.
  pub sync_eagerly: bool,
}
