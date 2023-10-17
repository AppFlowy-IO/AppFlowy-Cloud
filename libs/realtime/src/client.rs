use crate::entities::{ClientMessage, Connect, Disconnect, RealtimeMessage, RealtimeUser};
use std::fmt::{Display, Formatter};

use actix::{
  fut, Actor, ActorContext, ActorFutureExt, Addr, AsyncContext, ContextFutureSpawner, Handler,
  Recipient, Running, StreamHandler, WrapFuture,
};
use actix_web_actors::ws;
use bytes::Bytes;
use std::ops::Deref;

use crate::collaborate::{CollabPermission, CollabServer};
use crate::error::RealtimeError;

use actix_web_actors::ws::ProtocolError;
use database::collab::CollabStorage;
use realtime_entity::collab_msg::CollabMessage;
use std::time::{Duration, Instant};
use tracing::error;

pub struct ClientSession<U: Unpin + RealtimeUser, S: Unpin + 'static, P: Unpin + CollabPermission> {
  user: U,
  hb: Instant,
  pub server: Addr<CollabServer<S, U, P>>,
  heartbeat_interval: Duration,
  client_timeout: Duration,
}

impl<U, S, P> ClientSession<U, S, P>
where
  U: Unpin + RealtimeUser + Clone,
  S: CollabStorage + Unpin,
  P: CollabPermission + Unpin,
{
  pub fn new(
    user: U,
    server: Addr<CollabServer<S, U, P>>,
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

  fn forward_binary(&self, bytes: Bytes) -> Result<(), RealtimeError> {
    tracing::debug!("Receive binary message with len: {}", bytes.len());
    let message = RealtimeMessage::from_vec(bytes.to_vec())?;
    match CollabMessage::from_vec(&message.payload) {
      Ok(collab_msg) => {
        self.server.do_send(ClientMessage {
          business_id: message.business_id,
          user: self.user.clone(),
          content: collab_msg,
        });
      },
      Err(e) => {
        tracing::warn!("Parser realtime payload failed: {:?}", e);
      },
    }

    Ok(())
  }
}

impl<U, S, P> Actor for ClientSession<U, S, P>
where
  U: Unpin + RealtimeUser,
  S: Unpin + CollabStorage,
  P: CollabPermission + Unpin,
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

impl<U, S, P> Handler<RealtimeMessage> for ClientSession<U, S, P>
where
  U: Unpin + RealtimeUser,
  S: Unpin + CollabStorage,
  P: CollabPermission + Unpin,
{
  type Result = ();

  fn handle(&mut self, msg: RealtimeMessage, ctx: &mut Self::Context) {
    ctx.binary(msg);
  }
}

/// WebSocket message handler
impl<U, S, P> StreamHandler<Result<ws::Message, ws::ProtocolError>> for ClientSession<U, S, P>
where
  U: Unpin + RealtimeUser + Clone,
  S: Unpin + CollabStorage,
  P: CollabPermission + Unpin,
{
  fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
    let msg = match msg {
      Err(err) => {
        error!("Websocket stream error: {}", err);
        if let ProtocolError::Overflow = err {
          ctx.stop();
        }
        return;
      },
      Ok(msg) => msg,
    };

    match msg {
      ws::Message::Ping(msg) => {
        self.hb = Instant::now();
        ctx.pong(&msg);
      },
      ws::Message::Pong(_) => self.hb = Instant::now(),
      ws::Message::Text(_) => {},
      ws::Message::Binary(bytes) => {
        let _ = self.forward_binary(bytes);
      },
      ws::Message::Close(reason) => {
        ctx.close(reason);
        ctx.stop();
      },
      ws::Message::Continuation(_) => {},
      ws::Message::Nop => (),
    }
  }
}

/// A helper struct that wraps the [Recipient] type to implement the [Sink] trait
pub struct ClientWSSink(pub Recipient<RealtimeMessage>);
impl Deref for ClientWSSink {
  type Target = Recipient<RealtimeMessage>;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RealtimeUserImpl {
  pub uid: i64,
  pub uuid: String,
  pub device_id: String,
}

impl RealtimeUserImpl {
  pub fn new(uid: i64, uuid: String, device_id: String) -> Self {
    Self {
      uid,
      uuid,
      device_id,
    }
  }
}

impl Display for RealtimeUserImpl {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "uuid:{}|device_id:{}",
      self.uuid, self.device_id,
    ))
  }
}

impl RealtimeUser for RealtimeUserImpl {
  fn uid(&self) -> i64 {
    self.uid
  }
}
