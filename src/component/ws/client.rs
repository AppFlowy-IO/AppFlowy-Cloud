use crate::component::auth::LoggedUser;
use crate::component::ws::entities::{
  Connect, Disconnect, MessageDetail, MessagePayload, Socket, WebSocketMessage,
};
use crate::component::ws::server::CollabServer;
use crate::component::ws::{HEARTBEAT_INTERVAL, PING_TIMEOUT};
use actix::*;
use actix_http::ws::Message::*;
use actix_web::web::Data;
use actix_web_actors::ws;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub trait MessageReceiver: Send + Sync {
  fn receive(&self, data: WSClientData);
}

#[derive(Default)]
pub struct MessageReceivers {
  inner: HashMap<u8, Arc<dyn MessageReceiver>>,
}

impl MessageReceivers {
  pub fn new() -> Self {
    MessageReceivers::default()
  }

  pub fn insert(&mut self, channel: u8, receiver: Arc<dyn MessageReceiver>) {
    self.inner.insert(channel, receiver);
  }

  pub fn get(&self, source: u8) -> Option<&Arc<dyn MessageReceiver>> {
    self.inner.get(&source)
  }
}

#[allow(dead_code)]
pub struct WSClientData {
  pub(crate) socket: Socket,
  pub(crate) detail: MessageDetail,
}

pub struct WSClient {
  user: Arc<LoggedUser>,
  server: Addr<CollabServer>,
  msg_receivers: Data<MessageReceivers>,
  hb: Instant,
}

impl WSClient {
  pub fn new(
    user: LoggedUser,
    server: Addr<CollabServer>,
    msg_receivers: Data<MessageReceivers>,
  ) -> Self {
    Self {
      user: Arc::new(user),
      server,
      msg_receivers,
      hb: Instant::now(),
    }
  }

  fn hb(&self, ctx: &mut ws::WebsocketContext<Self>) {
    ctx.run_interval(HEARTBEAT_INTERVAL, |client, ctx| {
      if Instant::now().duration_since(client.hb) > PING_TIMEOUT {
        client.server.do_send(Disconnect {
          user: client.user.clone(),
        });
        ctx.stop();
      } else {
        ctx.ping(b"");
      }
    });
  }

  fn handle_binary_message(&self, bytes: Bytes, socket: Socket) {
    let MessagePayload { channel, detail } = MessagePayload::from_bytes(&bytes);
    match self.msg_receivers.get(channel) {
      None => {
        tracing::error!("Can't find the receiver for {:?}", channel);
      },
      Some(handler) => {
        let client_data = WSClientData { socket, detail };
        handler.receive(client_data);
      },
    }
  }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WSClient {
  fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
    match msg {
      Ok(Ping(msg)) => {
        self.hb = Instant::now();
        ctx.pong(&msg);
      },
      Ok(Pong(_msg)) => {
        // tracing::trace!("Receive {} pong {:?}", &self.session_id, &msg);
        self.hb = Instant::now();
      },
      Ok(Binary(bytes)) => {
        let socket = ctx.address().recipient();
        self.handle_binary_message(bytes, socket);
      },
      Ok(Text(_)) => {
        tracing::warn!("Receive unexpected text message");
      },
      Ok(Close(reason)) => {
        ctx.close(reason);
        ctx.stop();
      },
      Ok(ws::Message::Continuation(_)) => {},
      Ok(ws::Message::Nop) => {},
      Err(e) => {
        tracing::error!("WebSocketStream protocol error {:?}", e);
        ctx.stop();
      },
    }
  }
}

impl Handler<WebSocketMessage> for WSClient {
  type Result = ();

  fn handle(&mut self, msg: WebSocketMessage, ctx: &mut Self::Context) {
    ctx.binary(msg.0);
  }
}

impl Actor for WSClient {
  type Context = ws::WebsocketContext<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    self.hb(ctx);
    let socket = ctx.address().recipient();
    let connect = Connect {
      socket,
      user: self.user.clone(),
    };
    self
      .server
      .send(connect)
      .into_actor(self)
      .then(|res, _client, _ctx| {
        match res {
          Ok(Ok(_)) => tracing::trace!("Send connect message to server success"),
          Ok(Err(e)) => tracing::error!("Send connect message to server failed: {:?}", e),
          Err(e) => tracing::error!("Send connect message to server failed: {:?}", e),
        }
        fut::ready(())
      })
      .wait(ctx);
  }

  fn stopping(&mut self, _: &mut Self::Context) -> Running {
    self.server.do_send(Disconnect {
      user: self.user.clone(),
    });

    Running::Stop
  }
}
