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

use std::time::{Duration, Instant};
use storage::collab::CollabStorage;

pub struct CollabSession<U, S: Unpin + 'static> {
  user: U,
  hb: Instant,
  pub server: Addr<CollabServer<S>>,
  heartbeat_interval: Duration,
  client_timeout: Duration,
}

impl<U, S> CollabSession<U, S>
where
  U: Unpin + RealtimeUser + Clone,
  S: CollabStorage + Unpin,
{
  pub fn new(
    user: U,
    server: Addr<CollabServer<S>>,
    heartbeat_interval: Duration,
    client_timeout: Duration,
  ) -> Self {
    Self {
      user,
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
      Ok(message) => match CollabMessage::from_vec(&message.payload) {
        Ok(collab_msg) => {
          self.server.do_send(ClientMessage {
            business_id: message.business_id,
            user: self.user.clone(),
            content: collab_msg,
          });
        },
        Err(e) => tracing::error!("Parser CollabMessage failed: {:?}", e),
      },
      Err(e) => {
        tracing::error!("Parser binary message error: {:?}", e);
      },
    }
  }
}

impl<U, S> Actor for CollabSession<U, S>
where
  U: Unpin + RealtimeUser + Clone,
  S: Unpin + CollabStorage,
{
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

impl<U, S> Handler<ServerMessage> for CollabSession<U, S>
where
  U: Unpin + RealtimeUser,
  S: Unpin + CollabStorage,
{
  type Result = ();

  fn handle(&mut self, server_msg: ServerMessage, ctx: &mut Self::Context) {
    ctx.binary(RealtimeMessage::from(server_msg));
  }
}

/// WebSocket message handler
impl<U, S> StreamHandler<Result<ws::Message, ws::ProtocolError>> for CollabSession<U, S>
where
  U: Unpin + RealtimeUser + Clone,
  S: Unpin + CollabStorage,
{
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
pub struct ClientWebsocketSink(pub Recipient<ServerMessage>);
impl Deref for ClientWebsocketSink {
  type Target = Recipient<ServerMessage>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}
