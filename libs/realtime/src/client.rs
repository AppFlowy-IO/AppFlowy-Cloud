use crate::collaborate::{CollabAccessControl, CollabServer};
use crate::entities::{ClientMessage, Connect, Disconnect, RealtimeMessage, RealtimeUser};
use crate::error::RealtimeError;
use actix::{
  fut, Actor, ActorContext, ActorFutureExt, Addr, AsyncContext, ContextFutureSpawner, Handler,
  MailboxError, Recipient, Running, StreamHandler, WrapFuture,
};
use actix_web_actors::ws;
use actix_web_actors::ws::ProtocolError;
use bytes::Bytes;
use database::collab::CollabStorage;

use std::ops::Deref;
use std::time::{Duration, Instant};

use database::pg_row::AFUserNotification;
use realtime_entity::user::{AFUserChange, UserMessage};
use tracing::{debug, error, trace, warn};

pub struct ClientSession<
  U: Unpin + RealtimeUser,
  S: Unpin + 'static,
  AC: Unpin + CollabAccessControl,
> {
  session_id: String,
  user: U,
  hb: Instant,
  pub server: Addr<CollabServer<S, U, AC>>,
  heartbeat_interval: Duration,
  client_timeout: Duration,
  user_change_recv: Option<tokio::sync::mpsc::Receiver<AFUserNotification>>,
}

impl<U, S, AC> ClientSession<U, S, AC>
where
  U: Unpin + RealtimeUser + Clone,
  S: CollabStorage + Unpin,
  AC: CollabAccessControl + Unpin,
{
  pub fn new(
    user: U,
    user_change_recv: tokio::sync::mpsc::Receiver<AFUserNotification>,
    server: Addr<CollabServer<S, U, AC>>,
    heartbeat_interval: Duration,
    client_timeout: Duration,
  ) -> Self {
    Self {
      user,
      hb: Instant::now(),
      server,
      heartbeat_interval,
      client_timeout,
      user_change_recv: Some(user_change_recv),
      session_id: uuid::Uuid::new_v4().to_string(),
    }
  }

  fn hb(&self, ctx: &mut ws::WebsocketContext<Self>) {
    let session_id = self.session_id.clone();
    ctx.run_interval(self.heartbeat_interval, move |act, ctx| {
      if Instant::now().duration_since(act.hb) > act.client_timeout {
        let user = act.user.clone();
        warn!("{} heartbeat failed, disconnecting!", user);
        act.server.do_send(Disconnect {
          user,
          session_id: session_id.clone(),
        });
        ctx.stop();
        return;
      }

      ctx.ping(b"");
    });
  }

  fn forward_binary(&self, bytes: Bytes) -> Result<(), RealtimeError> {
    match RealtimeMessage::try_from(bytes) {
      Ok(message) => {
        debug!("Receive {} {}", self.user.uid(), message);
        let user = self.user.clone();
        if let Err(err) = self.server.try_send(ClientMessage { user, message }) {
          error!("Send message to server error: {:?}", err);
        }
      },
      Err(err) => {
        error!("Deserialize message error: {:?}", err);
      },
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
        while let Some(notification) = recv.recv().await {
          if let Some(user) = notification.payload {
            trace!("Receive user change: {:?}", user);

            // The RealtimeMessage uses bincode to do serde. But bincode doesn't support the Serde
            // deserialize_any method. So it needs to serialize the metadata to json string.
            let metadata = serde_json::to_string(&user.metadata).ok();
            let msg = UserMessage::ProfileChange(AFUserChange {
              uid: user.uid,
              name: user.name,
              email: user.email,
              metadata,
            });
            if let Err(err) = recipient.send(RealtimeMessage::User(msg)).await {
              match err {
                MailboxError::Closed => {
                  break;
                },
                MailboxError::Timeout => {
                  error!("User change message recipient send timeout");
                },
              }
            }
          }
        }
      });
    }

    self
      .server
      .send(Connect {
        socket: ctx.address().recipient(),
        user: self.user.clone(),
        session_id: self.session_id.clone(),
      })
      .into_actor(self)
      .then(|res, _session, ctx| {
        match res {
          Ok(Ok(_)) => {
            trace!("ws client send connect message to server success")
          },
          Ok(Err(err)) => {
            error!("ws client send connect message to server error: {:?}", err);
            ctx.stop();
          },
          Err(err) => {
            error!("ws client send connect message to server error: {:?}", err);
            ctx.stop();
          },
        }
        fut::ready(())
      })
      .wait(ctx);
  }

  fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
    // When the user is None which means the user is kicked off by the server, do not send
    // disconnect message to the server.
    let user = self.user.clone();
    trace!("{} stopping websocket connect", user);
    self.server.do_send(Disconnect {
      user,
      session_id: self.session_id.clone(),
    });
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
      RealtimeMessage::ServerKickedOff => ctx.stop(),
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
        debug!(
          "Websocket closing for ({:?}): {:?}",
          self.user.uid(),
          reason
        );
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
