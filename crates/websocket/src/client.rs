use crate::entities::{ClientMessage, Connect, Disconnect, ServerMessage, WSUser};
use crate::error::WSError;
use crate::CollabServer;
use actix::{
  fut, Actor, ActorContext, ActorFutureExt, Addr, AsyncContext, ContextFutureSpawner, Handler,
  Recipient, Running, StreamHandler, WrapFuture,
};
use actix_web_actors::ws;
use bytes::Bytes;

use collab_sync::msg::CollabMessage;
use futures_util::Sink;

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct CollabSession {
  user: Arc<WSUser>,
  hb: Instant,
  pub server: Addr<CollabServer>,
}

impl CollabSession {
  pub fn new(user: WSUser, server: Addr<CollabServer>) -> Self {
    Self {
      user: Arc::new(user),
      hb: Instant::now(),
      server,
    }
  }

  fn hb(&self, ctx: &mut ws::WebsocketContext<Self>) {
    ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
      if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
        act.server.do_send(Disconnect {
          user: act.user.clone(),
        });
        ctx.stop();
        return;
      }

      ctx.ping(b"");
    });
  }

  fn send_to_server(&self, bytes: Bytes) {
    match CollabMessage::from_vec(bytes.to_vec()) {
      Ok(collab_msg) => {
        self.server.do_send(ClientMessage {
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
            tracing::error!("Send connect message to server failed");
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

  fn handle(&mut self, msg: ServerMessage, ctx: &mut Self::Context) {
    ctx.binary(msg.collab_msg);
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
        self.send_to_server(bytes);
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

impl Sink<CollabMessage> for ClientSink {
  type Error = WSError;

  fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  fn start_send(self: Pin<&mut Self>, item: CollabMessage) -> Result<(), Self::Error> {
    self.0.do_send(ServerMessage { collab_msg: item });
    Ok(())
  }

  fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }
}
