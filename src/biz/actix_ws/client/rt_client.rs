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
use governor::clock::DefaultClock;
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use semver::Version;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, trace, warn};

const MAX_MESSAGES_PER_INTERVAL: usize = 10;
const RATE_LIMIT_INTERVAL: Duration = Duration::from_secs(1);

type WSRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>;
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
  rate_limiter: Arc<RwLock<WSRateLimiter>>,
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
    let rate_limiter = gen_rate_limiter(10);
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
      rate_limiter: Arc::new(RwLock::new(rate_limiter)),
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
          // Extract the user object from the notification payload.
          if let Some(user) = notification.payload {
            trace!("Receive user change: {:?}", user);
            // Since bincode serialization is used for RealtimeMessage but does not support the
            // Serde `deserialize_any` method, the user metadata is serialized into a JSON string.
            // This step ensures compatibility and flexibility for the metadata field.
            let metadata = serde_json::to_string(&user.metadata).ok();

            // Construct a UserMessage with the user's details, including the serialized metadata.
            let msg = UserMessage::ProfileChange(AFUserChange {
              uid: user.uid,
              name: user.name,
              email: user.email,
              metadata,
            });

            // Attempt to send the constructed message to the recipient. Handle errors appropriately,
            // notably breaking out of the loop if the recipient's mailbox is closed, indicating
            // that no further messages can be sent.
            if let Err(err) = recipient.send(RealtimeMessage::User(msg)).await {
              match err {
                MailboxError::Closed => {
                  // If the recipient's mailbox is closed, stop listening for further notifications.
                  break;
                },
                MailboxError::Timeout => {
                  // Log a timeout error if the message could not be sent within the expected time frame.
                  error!("User change message recipient send timeout");
                },
              }
            }
          }
        }
      });
    }

    // Asynchronously sends a `Connect` message to the server actor, indicating a new websocket connection attempt.
    // The `Connect` message includes the address of the current actor (as a recipient for further communication)
    // and the user details associated with this connection attempt.
    self.server
        .send(Connect {
          socket: ctx.address().recipient(),
          user: self.user.clone(),
        })
        // Converts the future into an actor future, allowing it to be handled within the actor context.
        .into_actor(self)
        // Process the server response. The result `res` can be either an acknowledgement (Ok) or an error (Err).
        // The `then` combinator is used to handle these outcomes and decide on subsequent actions.
        .then(|res, _session, ctx| {
          match res {
            // In case of successful connection acknowledgement from the server,
            // log a trace message indicating the successful connection attempt.
            Ok(Ok(_)) => {
              trace!("WebSocket client successfully sent connect message to server.")
            },
            // If the server responded with an error, or sending the message resulted in an error,
            // log the error and stop the current actor. This halts further processing in case of failure.
            Ok(Err(err)) | Err(err) => {
              error!("WebSocket client failed to send connect message to server: {:?}", err);
              ctx.stop();
            },
          }
          fut::ready(())
        })
        // Await the completion of the above future before proceeding, ensuring that the actor processes
        // it in sequence. This is necessary for handling the asynchronous send operation within the actor's context.
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
// self.rate_limiter.read().await.until_ready().fuse().await;

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

fn gen_rate_limiter(
  mut times_per_sec: u32,
) -> RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware> {
  if times_per_sec == 0 {
    times_per_sec = 1;
  }
  let quota = Quota::per_second(NonZeroU32::new(times_per_sec).unwrap());
  RateLimiter::direct(quota)
}
