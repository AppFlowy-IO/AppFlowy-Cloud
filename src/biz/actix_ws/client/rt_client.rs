use crate::biz::actix_ws::entities::{ClientMessage, Connect, Disconnect, RealtimeMessage};
use crate::biz::actix_ws::server::RealtimeServerActor;
use actix::{
  fut, Actor, ActorContext, ActorFutureExt, Addr, AsyncContext, ContextFutureSpawner, Handler,
  MailboxError, Recipient, Running, StreamHandler, WrapFuture,
};
use actix_web_actors::ws;
use actix_web_actors::ws::{CloseCode, CloseReason, ProtocolError};
use anyhow::anyhow;
use async_trait::async_trait;
use bytes::Bytes;
use collab_rt::error::RealtimeError;
use collab_rt::{RealtimeAccessControl, RealtimeClientWebsocketSink};
use collab_rt_entity::user::{AFUserChange, RealtimeUser, UserMessage};
use collab_rt_entity::SystemMessage;
use database::collab::CollabStorage;
use database::pg_row::AFUserNotification;
use semver::Version;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, error, trace, warn};

const MAX_MESSAGES_PER_INTERVAL: usize = 10;
const RATE_LIMIT_INTERVAL: Duration = Duration::from_secs(1);

pub struct RealtimeClient<S: Unpin + 'static, AC: Unpin + RealtimeAccessControl> {
  user: RealtimeUser,
  hb: Instant,
  pub server: Addr<RealtimeServerActor<S, AC>>,
  heartbeat_interval: Duration,
  client_timeout: Duration,
  user_change_recv: Option<tokio::sync::mpsc::Receiver<AFUserNotification>>,
  message_count: usize,
  interval_start: Instant,
  #[allow(dead_code)]
  client_version: Version,
}

impl<S, AC> RealtimeClient<S, AC>
where
  S: CollabStorage + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  pub fn new(
    user: RealtimeUser,
    user_change_recv: tokio::sync::mpsc::Receiver<AFUserNotification>,
    server: Addr<RealtimeServerActor<S, AC>>,
    heartbeat_interval: Duration,
    client_timeout: Duration,
    client_version: Version,
  ) -> Self {
    Self {
      user,
      hb: Instant::now(),
      server,
      heartbeat_interval,
      client_timeout,
      user_change_recv: Some(user_change_recv),
      message_count: 0,
      interval_start: Instant::now(),
      client_version,
    }
  }

  fn hb(&self, ctx: &mut ws::WebsocketContext<Self>) {
    ctx.run_interval(self.heartbeat_interval, move |act, ctx| {
      if Instant::now().duration_since(act.hb) > act.client_timeout {
        let user = act.user.clone();
        warn!(
          "User {} heartbeat failed, exceeding timeout limit of {} secs. Disconnecting!",
          user,
          act.client_timeout.as_secs()
        );

        act.server.do_send(Disconnect { user });
        ctx.stop();
        return;
      }

      ctx.ping(b"");
    });
  }

  async fn send_binary_to_server(
    user: RealtimeUser,
    server: Addr<RealtimeServerActor<S, AC>>,
    bytes: Bytes,
  ) -> Result<(), RealtimeError> {
    let message = tokio::task::spawn_blocking(move || {
      RealtimeMessage::decode(bytes.as_ref()).map_err(RealtimeError::Internal)
    })
    .await
    .map_err(|err| RealtimeError::Internal(err.into()))??;
    let mut client_message = Some(ClientMessage {
      user: user.clone(),
      message,
    });
    let mut attempts = 0;
    const MAX_RETRIES: usize = 3;
    const RETRY_DELAY: Duration = Duration::from_millis(500);
    while attempts < MAX_RETRIES {
      if let Some(message_to_send) = client_message.take() {
        match server.try_send(message_to_send) {
          Ok(_) => return Ok(()),
          Err(err) if attempts < MAX_RETRIES - 1 => {
            client_message = Some(err.into_inner());
            attempts += 1;
            sleep(RETRY_DELAY).await;
          },
          Err(err) => {
            return Err(RealtimeError::Internal(anyhow!(
              "Failed to send message to server after retries: {:?}",
              err
            )));
          },
        }
      } else {
        return Err(RealtimeError::Internal(anyhow!(
          "Unexpected empty client message"
        )));
      }
    }
    Ok(())
  }
}

impl<S, P> Actor for RealtimeClient<S, P>
where
  S: Unpin + CollabStorage,
  P: RealtimeAccessControl + Unpin,
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
    self.server.do_send(Disconnect { user });
    Running::Stop
  }
}

/// Handle message sent from the server
impl<S, AC> Handler<RealtimeMessage> for RealtimeClient<S, AC>
where
  S: Unpin + CollabStorage,
  AC: RealtimeAccessControl + Unpin,
{
  type Result = ();

  fn handle(&mut self, msg: RealtimeMessage, ctx: &mut Self::Context) {
    if let RealtimeMessage::System(SystemMessage::DuplicateConnection) = &msg {
      let reason = CloseReason {
        code: CloseCode::Normal,
        description: Some("Duplicate connection".to_string()),
      };
      ctx.close(Some(reason));
      ctx.stop();
    }

    match msg.encode() {
      Ok(data) => {
        ctx.binary(Bytes::from(data));
      },
      Err(err) => {
        error!("Error encoding message: {}", err);
      },
    }
  }
}

/// Handle the messages sent from the client
impl<S, AC> StreamHandler<Result<ws::Message, ws::ProtocolError>> for RealtimeClient<S, AC>
where
  S: Unpin + CollabStorage,
  AC: RealtimeAccessControl + Unpin,
{
  fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
    let now = Instant::now();
    if now.duration_since(self.interval_start) > RATE_LIMIT_INTERVAL {
      self.message_count = 0;
      self.interval_start = now;
    }

    self.message_count += 1;
    if self.message_count > MAX_MESSAGES_PER_INTERVAL {
      // TODO(nathan): inform the client to slow down
      return;
    }

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
        let fut = Self::send_binary_to_server(self.user.clone(), self.server.clone(), bytes);
        let act_fut = fut::wrap_future::<_, Self>(fut);
        ctx.spawn(act_fut.map(|res, _act, _ctx| {
          if let Err(e) = res {
            error!("Error forwarding binary message: {}", e);
          }
        }));
      },
      ws::Message::Close(reason) => {
        debug!("Websocket closing for ({:?}): {:?}", self.user.uid, reason);
        ctx.close(reason);
        ctx.stop();
      },
      ws::Message::Continuation(_) => {},
      ws::Message::Nop => (),
    }
  }
}

#[derive(Clone)]
pub struct RealtimeClientWebsocketSinkImpl(pub Recipient<RealtimeMessage>);

#[async_trait]
impl RealtimeClientWebsocketSink for RealtimeClientWebsocketSinkImpl {
  fn do_send(&self, message: RealtimeMessage) {
    self.0.do_send(message);
  }
}
