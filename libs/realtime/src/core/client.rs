use crate::entities::{
  ClientMessage, Connect, Disconnect, RealtimeMessage, RealtimeUser, ServerMessage,
};

use actix::{
  fut, Actor, ActorContext, ActorFutureExt, Addr, AsyncContext, ContextFutureSpawner, Handler,
  Recipient, Running, StreamHandler, WrapFuture,
};
use actix_web_actors::ws;
use bytes::Bytes;
use std::ops::Deref;

use crate::core::CollabServer;
use collab_sync_protocol::CollabMessage;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct CollabSession {
  user: Arc<RealtimeUser>,
  hb: Instant,
  pub server: Addr<CollabServer>,
  heartbeat_interval: Duration,
  client_timeout: Duration,
}

impl CollabSession {
  pub fn new(
    user: RealtimeUser,
    server: Addr<CollabServer>,
    heartbeat_interval: Duration,
    client_timeout: Duration,
  ) -> Self {
    Self {
      user: Arc::new(user),
      hb: Instant::now(),
      server,
      heartbeat_interval,
      client_timeout,
    }
  }

  fn hb(&self, ctx: &mut ws::WebsocketContext<Self>) {
    ctx.run_interval(self.heartbeat_interval, |act, ctx| {
      if Instant::now().duration_since(act.hb) > act.client_timeout {
        act.server.do_send(Disconnect {
          user: act.user.clone(),
        });
        ctx.stop();
        return;
      }

      ctx.ping(b"");
    });
  }

  fn forward_binary_to_ws_server(&self, bytes: Bytes) {
    match RealtimeMessage::from_vec(bytes.to_vec()) {
      Ok(ws_message) => {
        let collab_msg = CollabMessage::from_vec(&ws_message.payload).unwrap();
        self.server.do_send(ClientMessage {
          business_id: ws_message.business_id,
          user: self.user.clone(),
          collab_msg,
        });
      },
      Err(e) => {
        tracing::error!("Error parsing message: {:?}", e);
      },
    }
  }
}

impl Actor for CollabSession {
  type Context = ws::WebsocketContext<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    // start heartbeats otherwise server disconnects in 10 seconds
    self.hb(ctx);

    self
      .server
      .send(Connect {
        socket: ctx.address().recipient(),
        user: self.user.clone(),
      })
      .into_actor(self)
      .then(|res, _session, ctx| {
        match res {
          Ok(Ok(_)) => {
            tracing::trace!("Send connect message to server success")
          },
          _ => {
            tracing::error!("🔴Send connect message to server failed");
            ctx.stop();
          },
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

impl Handler<ServerMessage> for CollabSession {
  type Result = ();

  fn handle(&mut self, server_msg: ServerMessage, ctx: &mut Self::Context) {
    ctx.binary(RealtimeMessage::from(server_msg));
  }
}

/// WebSocket message handler
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for CollabSession {
  fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
    let msg = match msg {
      Err(_) => {
        ctx.stop();
        return;
      },
      Ok(msg) => msg,
    };

    match msg {
      ws::Message::Ping(msg) => {
        self.hb = Instant::now();
        ctx.pong(&msg);
      },
      ws::Message::Pong(_) => {
        self.hb = Instant::now();
      },
      ws::Message::Text(_) => {},
      ws::Message::Binary(bytes) => {
        self.forward_binary_to_ws_server(bytes);
      },
      ws::Message::Close(reason) => {
        ctx.close(reason);
        ctx.stop();
      },
      ws::Message::Continuation(_) => {
        ctx.stop();
      },
      ws::Message::Nop => (),
    }
  }
}

/// A helper struct that wraps the [Recipient] type to implement the [Sink] trait
pub struct ClientSink(pub Recipient<ServerMessage>);
impl Deref for ClientSink {
  type Target = Recipient<ServerMessage>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}
