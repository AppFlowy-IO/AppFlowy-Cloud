use actix::{
  Actor, ActorContext, AsyncContext, Handler, Message as ActixMessage, Recipient, Running,
  StreamHandler,
};
use actix_http::header::HeaderMap;
use actix_web_actors::ws;
use actix_web_actors::ws::ProtocolError;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::{channel, Sender};
use tracing::{error, trace};

use client_websocket::{connect_async, Message, WebSocketStream};

use crate::config::WSClientConfig;

#[derive(Clone, Debug, ActixMessage)]
#[rtype(result = "()")]
pub struct MessageEnvelope(pub Message);

pub struct RealtimeClient {
  url: String,
  header_map: HeaderMap,
  server_msg_sender: Sender<MessageEnvelope>,
  client_msg_sender: Sender<MessageEnvelope>,
}

impl RealtimeClient {
  pub fn new(header_map: HeaderMap, config: &WSClientConfig) -> Self {
    let (server_msg_sender, _) = channel(config.buffer_capacity);
    let (client_msg_sender, _) = channel(config.buffer_capacity);
    Self {
      url: config.url.clone(),
      header_map,
      server_msg_sender,
      client_msg_sender,
    }
  }

  pub async fn connect(&self) -> client_websocket::Result<()> {
    let conn_result = connect_async(&self.url, self.header_map.clone().into()).await;
    match conn_result {
      Ok(ws_stream) => {
        let (sink, stream) = ws_stream.split();
        self.spawn_pass_client_message_to_server(sink);
        self.spawn_receive_server_message(stream);
        Ok(())
      },
      Err(err) => Err(err),
    }
  }

  fn spawn_pass_client_message_to_server(&self, mut ws_sink: SplitSink<WebSocketStream, Message>) {
    let mut rx = self.client_msg_sender.subscribe();
    tokio::spawn(async move {
      while let Ok(msg) = rx.recv().await {
        match ws_sink.send(msg.0).await {
          Ok(_) => (),
          Err(e) => error!("Failed to send message to the websocket: {:?}", e),
        }
      }
    });
  }

  fn spawn_receive_server_message(&self, mut ws_stream: SplitStream<WebSocketStream>) {
    let server_msg_sender = self.server_msg_sender.clone();
    tokio::spawn(async move {
      while let Some(msg) = ws_stream.next().await {
        match msg {
          Ok(msg) => match server_msg_sender.send(MessageEnvelope(msg)) {
            Ok(_) => (),
            Err(e) => error!("Failed to send message to the channel: {:?}", e),
          },
          Err(e) => error!("Failed to receive message from the websocket: {:?}", e),
        }
      }
    });
  }
}

impl Actor for RealtimeClient {
  type Context = ws::WebsocketContext<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    let recipient: Recipient<MessageEnvelope> = ctx.address().recipient();
    let mut rx = self.server_msg_sender.subscribe();
    tokio::spawn(async move {
      while let Ok(msg_envelope) = rx.recv().await {
        match recipient.send(msg_envelope).await {
          Ok(_) => (),
          Err(e) => error!("Failed to send message to the websocket: {:?}", e),
        }
      }
    });
  }

  fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
    trace!("stopping websocket connect");
    Running::Stop
  }
}

impl Handler<MessageEnvelope> for RealtimeClient {
  type Result = ();

  fn handle(&mut self, msg_envelope: MessageEnvelope, ctx: &mut Self::Context) {
    match msg_envelope.0 {
      Message::Text(_) => {},
      Message::Binary(bytes) => ctx.binary(bytes),
      Message::Close(_close_frame) => {
        // TODO: Convert close frame to close reason
        ctx.close(None)
      },
      Message::Ping(bytes) => ctx.ping(bytes.as_slice()),
      Message::Pong(bytes) => ctx.pong(bytes.as_slice()),
    }
  }
}

/// Handle the messages sent from the client
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for RealtimeClient {
  fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
    if let Err(err) = &msg {
      if let ProtocolError::Overflow = err {
        ctx.stop();
      }
      return;
    }
    match msg.unwrap() {
      ws::Message::Ping(bytes) => {
        let _ = self
          .client_msg_sender
          .send(MessageEnvelope(Message::Ping(bytes.to_vec())));
      },
      ws::Message::Pong(bytes) => {
        let _ = self
          .client_msg_sender
          .send(MessageEnvelope(Message::Pong(bytes.to_vec())));
      },
      ws::Message::Text(_) => {},
      ws::Message::Binary(bytes) => {
        let _ = self
          .client_msg_sender
          .send(MessageEnvelope(Message::Binary(bytes.to_vec())));
      },
      ws::Message::Close(_close_reason) => {
        // TODO: Convert close reason to frame
        let _ = self
          .client_msg_sender
          .send(MessageEnvelope(Message::Close(None)));
        ctx.stop();
      },
      ws::Message::Continuation(_) => {},
      ws::Message::Nop => (),
    }
  }
}
