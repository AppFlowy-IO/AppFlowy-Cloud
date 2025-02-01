use super::{ConnectionError, Oid, WorkspaceId};
use bytes::Bytes;
use futures_util::SinkExt;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::watch::{Receiver, Sender};
use tokio_stream::wrappers::WatchStream;
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::{tungstenite, MaybeTlsStream};
use tokio_util::sync::CancellationToken;
use yrs::block::ClientID;

type WsConn = tokio_tungstenite::WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub struct Envelope {
  message: Bytes,
  ack: tokio::sync::oneshot::Sender<Result<(), ConnectionError>>,
}

pub struct WorkspaceNetworkController {
  inner: Arc<Inner>,
}

impl WorkspaceNetworkController {
  pub fn new(options: Options) -> Self {
    WorkspaceNetworkController {
      inner: Inner::new(options),
    }
  }

  pub fn status(&self) -> ConnectionStatus {
    self.inner.status_rx.borrow().clone()
  }

  pub fn observe_connection_state(&self) -> WatchStream<ConnectionStatus> {
    WatchStream::new(self.inner.status_rx.clone())
  }

  pub async fn connect(&self) -> Result<(), ConnectionError> {
    {
      let status = self.inner.status_rx.borrow();
      match &*status {
        ConnectionStatus::Connected { .. } => return Ok(()), // already connected
        ConnectionStatus::Connecting { .. } => { /* we're already connecting */ },
        ConnectionStatus::Disconnected { .. } => {
          // send signal to inner that we want to connect
          let cancel = self.inner.shutdown.child_token();
          self
            .inner
            .status_tx
            .send(ConnectionStatus::Connecting { cancel })
            .unwrap();
        },
      }
    }
    self
      .inner
      .status_rx
      .clone()
      .wait_for(|s| matches!(s, ConnectionStatus::Connected { .. }))
      .await
      .unwrap();
    Ok(())
  }

  #[inline]
  pub async fn disconnect(&self) -> Result<(), ConnectionError> {
    self.disconnect_with::<String>(None).await
  }

  pub async fn disconnect_with<S: Into<Arc<str>>>(
    &self,
    reason: Option<S>,
  ) -> Result<(), ConnectionError> {
    let status = self.inner.status_rx.borrow();
    match &*status {
      ConnectionStatus::Disconnected { .. } => Ok(()),
      ConnectionStatus::Connecting { cancel } | ConnectionStatus::Connected { cancel } => {
        cancel.cancel();
        let reason = reason.map(|s| s.into());
        self
          .inner
          .status_tx
          .send(ConnectionStatus::Disconnected { reason })
          .unwrap();
        Ok(())
      },
    }
  }

  async fn send_message(ws: &mut WsConn, message: Bytes) -> Result<(), ConnectionError> {
    ws.send(tungstenite::Message::binary(message)).await?;
    Ok(())
  }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Options {
  pub url: String,
  pub workspace_id: WorkspaceId,
  pub auth_token: String,
  pub device_id: String,
  pub client_id: ClientID,
}

struct Inner {
  options: Options,
  status_tx: tokio::sync::watch::Sender<ConnectionStatus>,
  status_rx: tokio::sync::watch::Receiver<ConnectionStatus>,
  message_tx: UnboundedSender<Envelope>,
  shutdown: CancellationToken,
}

impl Inner {
  fn new(options: Options) -> Arc<Self> {
    let (status_tx, status_rx) = tokio::sync::watch::channel(ConnectionStatus::default());
    let (message_tx, message_rx) = tokio::sync::mpsc::unbounded_channel();
    let shutdown = CancellationToken::new();
    let inner = Arc::new(Inner {
      options,
      status_rx,
      status_tx,
      message_tx,
      shutdown,
    });
    tokio::spawn(Self::init_remote_receiver_task(inner.clone(), message_rx));
    inner
  }

  async fn init_remote_receiver_task(
    inner: Arc<Inner>,
    mut message_rx: UnboundedReceiver<Envelope>,
  ) {
    while let ConnectionStatus::Connecting { cancel } = inner
      .status_rx
      .wait_for(|s| matches!(s, ConnectionStatus::Connecting { .. }))
      .await
      .unwrap()
      .clone()
    {
      'establish_new_conn: loop {
        match Self::establish_connection(&inner.options, cancel.clone()).await {
          Ok(Some(conn)) => {
            // successfully made a connection
            Self::handle_connection(&inner, conn, &mut message_rx, cancel.clone());
            inner
              .status_tx
              .send(ConnectionStatus::Connected { cancel })
              .unwrap();
            break;
          },
          Ok(None) => {
            // connection establishing has been cancelled
            inner
              .status_tx
              .send(ConnectionStatus::Disconnected { reason: None })
              .unwrap();
            break;
          },
          Err(err) => {
            // failed to make a connection, wait a second and retry
            tracing::error!("failed to establish WebSocket v2 connection: {}", err);
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue 'establish_new_conn; // retry connection
          },
        }
      }

      if inner.shutdown.is_cancelled() {
        // inner has been dropped
        return;
      }
    }
  }

  async fn establish_connection(
    options: &Options,
    cancel: CancellationToken,
  ) -> Result<Option<WsConn>, ConnectionError> {
    let url = format!("{}/ws/v2/{}", options.url, options.workspace_id);
    let req = http::Request::builder()
      .uri(url)
      .header("authorization", &options.auth_token)
      .header("device-id", &options.device_id)
      .header("client-id", &options.client_id.to_string())
      .header("sec-websocket-key", "foo")
      .header("upgrade", "websocket")
      .header("connection", "upgrade")
      .header("sec-websocket-version", 13)
      .body(())?;
    let fut = tokio_tungstenite::connect_async(req);
    tokio::select! {
      res = fut => {
        let (stream, _resp) = res?;
        Ok(Some(stream.into()))
      }
      _ = cancel.cancelled() => {
        Ok(None)
      }
    }
  }

  fn handle_connection(
    inner: &Inner,
    mut conn: WsConn,
    messages: &mut UnboundedReceiver<Envelope>,
    cancel: CancellationToken,
  ) {
    todo!()
  }
}

impl Drop for Inner {
  fn drop(&mut self) {
    self.shutdown.cancel();
  }
}

#[derive(Debug, Clone)]
pub enum ConnectionStatus {
  Disconnected { reason: Option<Arc<str>> },
  Connecting { cancel: CancellationToken },
  Connected { cancel: CancellationToken },
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
