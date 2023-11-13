use crate::collaborate::{CollabAccessControl, CollabServer};
use crate::entities::{ClientMessage, Connect, Disconnect, RealtimeMessage, RealtimeUser};
use crate::error::RealtimeError;
use actix::{
  fut, Actor, ActorContext, ActorFutureExt, Addr, AsyncContext, ContextFutureSpawner, Handler,
  Recipient, Running, StreamHandler, WrapFuture,
};
use actix_web_actors::ws;
use actix_web_actors::ws::ProtocolError;
use bytes::Bytes;
use database::collab::CollabStorage;

use std::ops::Deref;
use std::time::{Duration, Instant};
use tokio::sync::broadcast::Receiver;

use database_entity::pg_row::AFUserNotification;
use realtime_entity::user::{AFUserChange, UserMessage};
use tracing::{error, trace};

pub struct ClientSession<
  U: Unpin + RealtimeUser,
  S: Unpin + 'static,
  AC: Unpin + CollabAccessControl,
> {
  user: Option<U>,
  hb: Instant,
  pub server: Addr<CollabServer<S, U, AC>>,
  heartbeat_interval: Duration,
  client_timeout: Duration,
  user_change_recv: Option<Receiver<AFUserNotification>>,
}

impl<U, S, AC> ClientSession<U, S, AC>
where
  U: Unpin + RealtimeUser + Clone,
  S: CollabStorage + Unpin,
  AC: CollabAccessControl + Unpin,
{
  pub fn new(
    user: U,
    user_change_recv: Receiver<AFUserNotification>,
    server: Addr<CollabServer<S, U, AC>>,
    heartbeat_interval: Duration,
    client_timeout: Duration,
  ) -> Self {
    Self {
      user: Some(user),
      hb: Instant::now(),
      server,
      heartbeat_interval,
      client_timeout,
      user_change_recv: Some(user_change_recv),
    }
  }

  fn hb(&self, ctx: &mut ws::WebsocketContext<Self>) {
    ctx.run_interval(self.heartbeat_interval, |act, ctx| {
      if Instant::now().duration_since(act.hb) > act.client_timeout {
        if let Some(user) = act.user.clone() {
          act.server.do_send(Disconnect { user });
        }
        ctx.stop();
        return;
      }

      ctx.ping(b"");
    });
  }

  fn forward_binary(&self, bytes: Bytes) -> Result<(), RealtimeError> {
    tracing::debug!("Receive binary: {}", bytes.len());
    if let Some(user) = self.user.clone() {
      match RealtimeMessage::try_from(bytes) {
        Ok(message) => {
          self.server.do_send(ClientMessage { user, message });
        },
        Err(err) => {
          error!("Deserialize message error: {:?}", err);
        },
      }
    }
    Ok(())
  }
}

impl<U, S, P> Actor for ClientSession<U, S, P>
where
  U: Unpin + RealtimeUser,
  S: Unpin + CollabStorage,
  P: CollabAccessControl + Unpin,
{
  type Context = ws::WebsocketContext<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    self.hb(ctx);
    let recipient = ctx.address().recipient();
    if let Some(mut recv) = self.user_change_recv.take() {
      actix::spawn(async move {
        while let Ok(notification) = recv.recv().await {
          if let Some(user) = notification.payload {
            trace!("Receive user change: {:?}", user);

            // The RealtimeMessage uses bincode to do serde. But bincode doesn't support the Serde
            // deserialize_any method. So it needs to serialize the metadata to json string.
            let metadata = serde_json::to_string(&user.metadata).ok();
            let msg = UserMessage::ProfileChange(AFUserChange {
              name: user.name,
              email: user.email,
              metadata,
            });
            if let Err(err) = recipient.send(RealtimeMessage::User(msg)).await {
              error!("Send user change message error: {:?}", err);
            }
          }
        }
      });
    }

    if let Some(user) = self.user.clone() {
      self
        .server
        .send(Connect {
          socket: ctx.address().recipient(),
          user,
        })
        .into_actor(self)
        .then(|res, _session, ctx| {
          match res {
            Ok(Ok(_)) => {
              tracing::trace!("Send connect message to server success")
            },
            _ => {
              error!("🔴Send connect message to server failed");
              ctx.stop();
            },
          }
          fut::ready(())
        })
        .wait(ctx);
    }
  }

  fn stopping(&mut self, _: &mut Self::Context) -> Running {
    // When the user is None which means the user is kicked off by the server, do not send
    // disconnect message to the server.
    if let Some(user) = self.user.clone() {
      self.server.do_send(Disconnect { user });
    }
    Running::Stop
  }
}

impl<U, S, AC> Handler<RealtimeMessage> for ClientSession<U, S, AC>
where
  U: Unpin + RealtimeUser,
  S: Unpin + CollabStorage,
  AC: CollabAccessControl + Unpin,
{
  type Result = ();

  fn handle(&mut self, msg: RealtimeMessage, ctx: &mut Self::Context) {
    match &msg {
      RealtimeMessage::Collab(_) => ctx.binary(msg),
      RealtimeMessage::User(_) => ctx.binary(msg),
      RealtimeMessage::ServerKickedOff => {
        // The server will send this message to the client when the client is kicked out. So
        // set the current user to None and stop the session.
        self.user.take();
        ctx.stop()
      },
    }
  }
}

/// Handle the messages sent from the client
impl<U, S, AC> StreamHandler<Result<ws::Message, ws::ProtocolError>> for ClientSession<U, S, AC>
where
  U: Unpin + RealtimeUser + Clone,
  S: Unpin + CollabStorage,
  AC: CollabAccessControl + Unpin,
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
