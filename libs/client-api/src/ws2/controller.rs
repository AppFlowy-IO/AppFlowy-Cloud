use super::{ConnectionError, Oid, WorkspaceId};
use bytes::Bytes;
use client_websocket::WebSocketStream;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::http;

pub struct Message {
  oid: Oid,
  data: Bytes,
  ack: tokio::sync::oneshot::Sender<Result<(), ConnectionError>>,
}

#[derive(Clone)]
pub struct WorkspaceNetworkController {
  workspace_id: WorkspaceId,
  queue: UnboundedSender<Message>,
}

impl WorkspaceNetworkController {
  pub async fn connect(options: Options) -> Result<Self, ConnectionError> {
    let (queue, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut conn = Self::establish_connection(&options)?;
    tokio::spawn(async move {
      while let Some(msg) = rx.recv().await {
        while let Err(err) = Self::send_message(&mut conn, &msg).await {
          tracing::error!("failed to send message: {}", err);
          conn = Self::establish_connection(&options).await?;
        }
      }
    });
    Ok(Self {
      queue,
      workspace_id: options.workspace_id,
    })
  }

  pub async fn send<D>(&self, oid: Oid, data: D) -> Result<(), ConnectionError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let msg = Message {
      oid,
      data: data.into_bytes(),
      ack: tx,
    };
    self.queue.send(msg).unwrap(); // as long as current object exists, the receiver will exist
    rx.await
  }

  async fn establish_connection(options: &Options) -> Result<WebSocketStream, ConnectionError> {
    let url = format!("{}/ws/v2/{}", options.url, options.workspace_id);
    let req = http::Request::builder()
      .uri(url)
      .header("authorization", &options.auth_token)
      .header("device-id", &options.device_id)
      .header("sec-websocket-key", "foo")
      .header("upgrade", "websocket")
      .header("connection", "upgrade")
      .header("sec-websocket-version", 13)
      .body(())?;
    let (stream, _resp) = tokio_tungstenite::connect_async(req).await?;
    Ok(stream.into())
  }

  async fn send_message(conn: &mut WebSocketStream, msg: &Message) -> Result<(), ConnectionError> {
    todo!()
  }
}

pub struct Options {
  pub url: String,
  pub workspace_id: WorkspaceId,
  pub auth_token: String,
  pub device_id: String,
}
