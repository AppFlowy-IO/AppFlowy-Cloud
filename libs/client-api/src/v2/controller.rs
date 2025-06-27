use super::db::{Db, DbHolder};
use super::{ChangedCollab, ObjectId, WorkspaceId};
use crate::entity::CollabType;
use crate::sync_trace;
use crate::v2::actor::{WorkspaceAction, WorkspaceControllerActor, WsConn};
use crate::v2::conn_retry::{ReconnectTarget, ReconnectionManager};
use app_error::ErrorCode;
use appflowy_proto::{Rid, WorkspaceNotification};
use async_trait::async_trait;
use collab::preclude::Collab;
use collab_rt_protocol::CollabRef;
use futures_core::Stream;
use futures_util::stream::SplitSink;
use shared_entity::response::AppResponseError;
use std::fmt::{Display, Formatter};
use std::sync::{Arc, Weak};
use tokio::sync::Mutex;
use tokio::time::interval;
use tokio_stream::wrappers::WatchStream;
use tokio_stream::StreamExt;
use tokio_tungstenite::tungstenite::error::ProtocolError;
use tokio_tungstenite::tungstenite::{Error, Message};
use tokio_util::sync::CancellationToken;
use tracing::instrument;
use yrs::block::ClientID;

#[derive(Clone)]
pub struct WorkspaceController {
  actor: Arc<WorkspaceControllerActor>,
  connection_manager: Arc<ReconnectionManager>,
}

impl WorkspaceController {
  pub fn new(options: Options, workspace_db_path: &str) -> anyhow::Result<Self> {
    let db = Db::open(options.workspace_id, options.uid, workspace_db_path)?;
    Self::new_with_db(options, db)
  }

  pub fn new_with_rocksdb<T: Into<DbHolder>>(options: Options, db: T) -> anyhow::Result<Self> {
    let db = Db::open_with_rocksdb(options.workspace_id, options.uid, db)?;
    Self::new_with_db(options, db)
  }

  fn new_with_db(options: Options, db: Db) -> anyhow::Result<Self> {
    let last_message_id = db.last_message_id()?;
    let actor = WorkspaceControllerActor::new(db, options, last_message_id);

    let conn_status = actor.status_channel().clone();
    let connection_manager = Arc::new(ReconnectionManager::new(actor.clone()));
    spawn_reconnection(Arc::downgrade(&connection_manager), conn_status);

    Ok(Self {
      actor,
      connection_manager,
    })
  }

  pub fn subscribe_changed_collab(&self) -> tokio::sync::broadcast::Receiver<ChangedCollab> {
    self.actor.subscribe_changed_collab()
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

  pub fn subscribe_connect_state(&self) -> impl Stream<Item = ConnectState> {
    let status_rx = self.actor.status_channel().clone();
    WatchStream::new(status_rx).map(|status| ConnectState::from(&status))
  }

  pub fn subscribe_notification(&self) -> tokio::sync::broadcast::Receiver<WorkspaceNotification> {
    self.actor.subscribe_notification()
  }

  pub async fn connect(&self, access_token: String) -> anyhow::Result<()> {
    if access_token.is_empty() {
      return Err(anyhow::anyhow!("access token is empty"));
    }

    self
      .connection_manager
      .set_access_token(access_token.clone());

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

  /// Binds a collaboration object to the actor and loads its data if needed.
  /// This function sets up the necessary callbacks and observers to handle
  /// collaboration updates and awareness changes.
  ///
  /// # Arguments
  ///
  /// * `actor`: Reference to the workspace controller actor managing the collaboration
  /// * `collab_ref`: Reference to the collaboration object to be bound
  /// * `collab_type`: The type of the collaboration (document, folder, etc.)
  pub async fn bind_and_cache_collab_ref(
    &self,
    collab_ref: &CollabRef,
    collab_type: CollabType,
  ) -> anyhow::Result<()> {
    WorkspaceControllerActor::bind_and_cache_collab_ref(&self.actor, collab_ref, collab_type).await
  }

  pub fn bind(&self, collab: &mut Collab, collab_type: CollabType) -> anyhow::Result<()> {
    WorkspaceControllerActor::bind(&self.actor, collab, collab_type)
  }

  pub async fn cache_collab_ref(
    &self,
    object_id: ObjectId,
    collab_ref: &CollabRef,
    collab_type: CollabType,
  ) -> anyhow::Result<()> {
    self
      .actor
      .cache_collab_ref(object_id, collab_ref, collab_type);
    Ok(())
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

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum DisconnectedReason {
  /// When disconnect reason is unexpected. ReconnectionManager will try to reconnect after a period of time
  Unexpected(Arc<str>),
  ResetWithoutClosingHandshake,
  MessageDecode(Arc<str>),
  ReachMaximumRetry,
  MessageLoopEnd(Arc<str>),
  CannotHandleReceiveMessage(Arc<str>),
  UserDisconnect(Arc<str>),
  ServerForceClose,
  Unauthorized(Arc<str>),
  PingTimeout,
}

impl Display for DisconnectedReason {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      DisconnectedReason::Unexpected(reason) => write!(f, "unexpected: {}", reason),
      DisconnectedReason::ResetWithoutClosingHandshake => {
        write!(f, "reset without closing handshake")
      },
      DisconnectedReason::MessageDecode(reason) => write!(f, "message decode: {}", reason),
      DisconnectedReason::ReachMaximumRetry => write!(f, "reach maximum retry"),
      DisconnectedReason::MessageLoopEnd(reason) => write!(f, "message loop end: {}", reason),
      DisconnectedReason::CannotHandleReceiveMessage(reason) => {
        write!(f, "cannot handle receive message: {}", reason)
      },
      DisconnectedReason::UserDisconnect(reason) => write!(f, "user disconnect: {}", reason),
      DisconnectedReason::Unauthorized(reason) => write!(f, "unauthorized: {}", reason),
      DisconnectedReason::ServerForceClose => write!(f, "server force close"),
      DisconnectedReason::PingTimeout => write!(f, "ping timeout"),
    }
  }
}

impl From<appflowy_proto::Error> for DisconnectedReason {
  fn from(value: appflowy_proto::Error) -> Self {
    DisconnectedReason::MessageDecode(value.to_string().into())
  }
}

impl From<tokio_tungstenite::tungstenite::Error> for DisconnectedReason {
  fn from(value: Error) -> Self {
    match value {
      Error::Io(err) => DisconnectedReason::Unexpected(err.to_string().into()),
      Error::Protocol(p) => match p {
        ProtocolError::ResetWithoutClosingHandshake => {
          DisconnectedReason::ResetWithoutClosingHandshake
        },
        _ => DisconnectedReason::Unexpected(format!("{:?}", p).into()),
      },
      _ => DisconnectedReason::MessageLoopEnd(value.to_string().into()),
    }
  }
}

impl From<AppResponseError> for DisconnectedReason {
  fn from(value: AppResponseError) -> Self {
    match value.code {
      ErrorCode::UserUnAuthorized => DisconnectedReason::Unauthorized(value.message.into()),
      _ => DisconnectedReason::Unexpected(value.message.into()),
    }
  }
}

impl DisconnectedReason {
  pub fn retriable(&self) -> bool {
    matches!(
      self,
      Self::Unexpected(..) | Self::ResetWithoutClosingHandshake
    )
  }

  pub fn retriable_when_editing(&self) -> bool {
    matches!(
      self,
      Self::Unexpected(..)
        | Self::ResetWithoutClosingHandshake
        | DisconnectedReason::Unauthorized(_)
        | DisconnectedReason::ReachMaximumRetry
    )
  }
}

#[derive(Debug, Clone)]
pub enum ConnectionStatus {
  Disconnected {
    reason: Option<DisconnectedReason>,
  },
  Connecting {
    cancel: CancellationToken,
  },
  Connected {
    sink: Arc<Mutex<SplitSink<WsConn, Message>>>,
    cancel: CancellationToken,
  },
  StartReconnect,
}

impl ConnectionStatus {
  pub fn disconnected_reason(&self) -> &Option<DisconnectedReason> {
    match self {
      ConnectionStatus::Disconnected { reason } => reason,
      _ => &None,
    }
  }
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
      } => write!(f, "disconnected: {:?}, ", reason),
      ConnectionStatus::Connecting { .. } => write!(f, "connecting"),
      ConnectionStatus::Connected { .. } => write!(f, "connected"),
      ConnectionStatus::StartReconnect => write!(f, "start reconnect"),
    }
  }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ConnectState {
  Disconnected { reason: Option<DisconnectedReason> },
  Connecting,
  Connected,
}

impl From<&ConnectionStatus> for ConnectState {
  fn from(value: &ConnectionStatus) -> Self {
    match value {
      ConnectionStatus::Disconnected { reason } => ConnectState::Disconnected {
        reason: reason.clone(),
      },
      ConnectionStatus::Connecting { .. } => ConnectState::Connecting,
      ConnectionStatus::Connected { .. } => ConnectState::Connected,
      ConnectionStatus::StartReconnect => ConnectState::Connecting,
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

#[async_trait]
impl ReconnectTarget for WorkspaceControllerActor {
  fn status_channel(&self) -> &tokio::sync::watch::Receiver<ConnectionStatus> {
    self.status_channel()
  }

  async fn attempt_connect(self: Arc<Self>, token: String) -> Result<(), AppResponseError> {
    WorkspaceControllerActor::handle_connect(&self, token).await
  }

  #[instrument(level = "trace", skip_all)]
  fn set_disconnected(&self, reason: DisconnectedReason) {
    self.set_connection_status(ConnectionStatus::Disconnected {
      reason: Some(reason),
    });
  }
}

pub fn spawn_reconnection(
  manager: Weak<ReconnectionManager>,
  mut connect_status_rx: tokio::sync::watch::Receiver<ConnectionStatus>,
) {
  tokio::spawn(async move {
    // Using interval to check connection status every 10 minutes. This ensures connection
    // monitoring continues even when the app is running in the background.
    let mut interval = interval(std::time::Duration::from_secs(600));
    interval.tick().await;

    loop {
      tokio::select! {
        result = connect_status_rx.changed() => {
          if result.is_err() {
            sync_trace!("connection state change dropped");
            break;
          }

          match manager.upgrade() {
            None => break ,
            Some(manager) => {
              check_and_reconnect(&manager, &connect_status_rx);
            }
          }
        }
        _ = interval.tick() => {
          match manager.upgrade() {
            None => break ,
            Some(manager) => {
              check_and_reconnect(&manager, &connect_status_rx);
            }
          }
        }
      }
    }
  });
}

fn check_and_reconnect(
  manager: &Arc<ReconnectionManager>,
  connect_status_rx: &tokio::sync::watch::Receiver<ConnectionStatus>,
) {
  sync_trace!(
    "check current connection status:{:?}",
    connect_status_rx.borrow()
  );
  match &*connect_status_rx.borrow() {
    ConnectionStatus::Disconnected {
      reason: Some(reason),
    } => {
      if reason.retriable() {
        manager.trigger_reconnect(&reason.to_string());
      }
    },
    ConnectionStatus::Disconnected { reason: None } => {},
    ConnectionStatus::Connecting { .. } => {},
    ConnectionStatus::Connected { .. } => {},
    ConnectionStatus::StartReconnect => {
      manager.trigger_reconnect("start reconnect");
    },
  }
}
