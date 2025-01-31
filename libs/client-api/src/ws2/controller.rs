use super::{ConnectionError, Oid, WorkspaceId};
use futures_util::SinkExt;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::{tungstenite, MaybeTlsStream};
use yrs::block::ClientID;
use yrs::encoding::write::Write;
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};

type WsConn = tokio_tungstenite::WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub struct Envelope {
  message: Vec<u8>,
  ack: tokio::sync::oneshot::Sender<Result<(), ConnectionError>>,
}

#[derive(Clone)]
pub struct WorkspaceNetworkController {
  workspace_id: WorkspaceId,
  queue: UnboundedSender<Envelope>,
}

impl WorkspaceNetworkController {
  pub async fn connect(options: Options) -> Result<Self, ConnectionError> {
    let (queue, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut conn = Self::establish_connection(&options)?;
    tokio::spawn(async move {
      while let Some(envelope) = rx.recv().await {
        match Self::send_message(&mut conn, envelope.message).await {
          Ok(_) => {
            let _ = envelope.ack.send(Ok(()));
          },
          Err(err) => {
            let _ = envelope.ack.send(Err(err));
            break;
          },
        }
      }
    });
    Ok(Self {
      queue,
      workspace_id: options.workspace_id,
    })
  }

  pub async fn send(&self, oid: Oid, message: yrs::sync::Message) -> Result<(), ConnectionError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut encoder = EncoderV1::new();
    encoder.write_string(&oid.to_string());
    message.encode(&mut encoder);
    let msg = Envelope {
      message: encoder.to_vec(),
      ack: tx,
    };
    self.queue.send(msg).unwrap(); // as long as current object exists, the receiver will exist
    rx.await.unwrap()
  }

  async fn establish_connection(options: &Options) -> Result<WsConn, ConnectionError> {
    let url = format!("{}/ws/v2/{}", options.url, options.workspace_id);
    let req = http::Request::builder()
      .uri(url)
      .header("authorization", &options.auth_token)
      .header("device-id", &options.device_id)
      .header("client-id", &options.client_id)
      .header("sec-websocket-key", "foo")
      .header("upgrade", "websocket")
      .header("connection", "upgrade")
      .header("sec-websocket-version", 13)
      .body(())?;
    let (stream, _resp) = tokio_tungstenite::connect_async(req).await?;
    Ok(stream.into())
  }

  async fn send_message(ws: &mut WsConn, message: Vec<u8>) -> Result<(), ConnectionError> {
    ws.send(tungstenite::Message::Binary(message)).await?;
    Ok(())
  }
}

pub struct Options {
  pub url: String,
  pub workspace_id: WorkspaceId,
  pub auth_token: String,
  pub device_id: String,
  pub client_id: ClientID,
}
