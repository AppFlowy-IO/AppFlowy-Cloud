use crate::actix_ws::entities::{ClientMessage, Connect, Disconnect, RealtimeMessage};
use crate::error::RealtimeError;
use crate::RealtimeClientWebsocketSink;
use actix::{
  fut, Actor, ActorContext, ActorFutureExt, Addr, AsyncContext, Context, ContextFutureSpawner,
  Handler, MailboxError, Recipient, ResponseFuture, Running, StreamHandler, WrapFuture,
};
use actix_web_actors::ws;
use actix_web_actors::ws::{CloseCode, CloseReason, ProtocolError, WebsocketContext};
use anyhow::anyhow;
use async_trait::async_trait;
use bytes::Bytes;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::SystemMessage;
use governor::clock::DefaultClock;
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use semver::Version;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{debug, error, trace, warn};

pub type HandlerResult = ResponseFuture<anyhow::Result<(), RealtimeError>>;
pub trait RealtimeServer:
  Actor<Context = Context<Self>>
  + Handler<ClientMessage, Result = HandlerResult>
  + Handler<Connect, Result = HandlerResult>
  + Handler<Disconnect, Result = HandlerResult>
{
}

type BinaryRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>;
pub struct RealtimeClient<S: RealtimeServer> {
  user: RealtimeUser,
  hb: Instant,
  pub server: Addr<S>,
  heartbeat_interval: Duration,
  client_timeout: Duration,
  external_source: Option<mpsc::Receiver<RealtimeMessage>>,
  #[allow(dead_code)]
  /// Indicates the version of the client, used for compatibility checks with the server.
  client_version: Version,
  /// To prevent overwhelming the server with too many messages at once, each client has a rate-limiting
  /// mechanism. This limits the number of messages a client can send per second, ensuring the server's
  /// mailbox does not get full from receiving too many messages at the same time.
  binary_rate_limiter: Arc<BinaryRateLimiter>,
}

impl<S> RealtimeClient<S>
where
  S: RealtimeServer,
{
  pub fn new(
    user: RealtimeUser,
    server: Addr<S>,
    heartbeat_interval: Duration,
    client_timeout: Duration,
    client_version: Version,
    external_source: mpsc::Receiver<RealtimeMessage>,
    rate_limit_times_per_sec: u32,
  ) -> Self {
    let rate_limiter = gen_rate_limiter(rate_limit_times_per_sec);
    Self {
      user,
      hb: Instant::now(),
      server,
      heartbeat_interval,
      client_timeout,
      external_source: Some(external_source),
      client_version,
      binary_rate_limiter: Arc::new(rate_limiter),
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

  pub fn try_send(&self, message: RealtimeMessage) -> Result<(), RealtimeError> {
    if self.binary_rate_limiter.check().is_err() {
      trace!("Rate limit exceeded for user: {}", self.user);
      return Err(RealtimeError::TooManyMessage(self.user.to_string()));
    }

    self
      .server
      .try_send(ClientMessage {
        user: self.user.clone(),
        message,
      })
      .map_err(|err| RealtimeError::Internal(err.into()))
  }
}

impl<S> RealtimeClient<S>
where
  S: RealtimeServer,
{
  fn handle_binary(&mut self, ctx: &mut WebsocketContext<RealtimeClient<S>>, bytes: Bytes) {
    // Immediately return if rate limit is exceeded.
    if let Err(e) = self.binary_rate_limiter.check() {
      trace!("Rate limit exceeded for user: {}, error: {}", self.user, e);
      return;
    }
    let server = self.server.clone();
    let user = self.user.clone();

    let fut = async move {
      match tokio::task::spawn_blocking(move || RealtimeMessage::decode(&bytes)).await {
        Ok(Ok(decoded_message)) => {
          let mut client_message = Some(ClientMessage {
            user,
            message: decoded_message,
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
                  return Err(anyhow!(
                    "Failed to send message to server after retries: {:?}",
                    err
                  ));
                },
              }
            } else {
              return Err(anyhow!("Unexpected empty client message"));
            }
          }
          Ok(())
        },
        Ok(Err(decode_err)) => Err(anyhow!("Error decoding message: {}", decode_err)),
        Err(spawn_err) => Err(anyhow!("Error spawning blocking task: {}", spawn_err)),
      }
    };

    let act_fut = fut::wrap_future::<_, Self>(fut);
    ctx.spawn(act_fut.map(|res, _act, _ctx| {
      if let Err(e) = res {
        error!("{}", e);
      }
    }));
  }

  fn handle_ping(&mut self, ctx: &mut WebsocketContext<RealtimeClient<S>>, msg: &Bytes) {
    self.hb = Instant::now();
    ctx.pong(msg);
  }

  fn handle_close(
    &mut self,
    ctx: &mut WebsocketContext<RealtimeClient<S>>,
    reason: Option<CloseReason>,
  ) {
    debug!("Websocket closing for ({:?}): {:?}", self.user.uid, reason);
    ctx.close(reason);
    ctx.stop();
  }
}

impl<S> Actor for RealtimeClient<S>
where
  S: RealtimeServer,
{
  type Context = ws::WebsocketContext<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    self.hb(ctx);
    let recipient = ctx.address().recipient();
    if let Some(mut external_source) = self.external_source.take() {
      actix::spawn(async move {
        while let Some(message) = external_source.recv().await {
          // Attempt to send the constructed message to the recipient. Handle errors appropriately,
          // notably breaking out of the loop if the recipient's mailbox is closed, indicating
          // that no further messages can be sent.
          if let Err(err) = recipient.send(message).await {
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
      });
    } else {
      error!("External source is empty, it should be only called once");
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
        .then(|res, _session, ctx| {
          match res {
            // In case of successful connection acknowledgement from the server,
            // log a trace message indicating the successful connection attempt.
            Ok(Ok(_)) => {
              trace!("WebSocket client successfully sent connect message to server.")
            },
            // If the server responded with an error, or sending the message resulted in an error,
            // log the error and stop the current actor.
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
impl<S> Handler<RealtimeMessage> for RealtimeClient<S>
where
  S: RealtimeServer,
{
  type Result = ();

  fn handle(&mut self, message: RealtimeMessage, ctx: &mut Self::Context) {
    match message.encode() {
      Ok(data) => ctx.binary(Bytes::from(data)),
      Err(err) => error!("Error encoding message: {}", err),
    }

    if let RealtimeMessage::System(SystemMessage::DuplicateConnection) = &message {
      let reason = CloseReason {
        code: CloseCode::Normal,
        description: Some("Duplicate connection".to_string()),
      };
      ctx.close(Some(reason));
    }
  }
}

/// Handle the messages sent from the client
impl<S> StreamHandler<Result<ws::Message, ws::ProtocolError>> for RealtimeClient<S>
where
  S: RealtimeServer,
{
  fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
    if let Err(err) = &msg {
      if let ProtocolError::Overflow = err {
        ctx.stop();
      }
      return;
    }

    match msg.unwrap() {
      ws::Message::Ping(msg) => self.handle_ping(ctx, &msg),
      ws::Message::Pong(_) => self.hb = Instant::now(),
      ws::Message::Text(_) => {},
      ws::Message::Binary(bytes) => self.handle_binary(ctx, bytes),
      ws::Message::Close(reason) => self.handle_close(ctx, reason),
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

#[cfg(test)]
mod tests {
  use std::time::Duration;
  use tokio::time::sleep;

  #[tokio::test]
  async fn rate_limit_test() {
    let rate_limiter = super::gen_rate_limiter(10);
    for i in 0..=10 {
      if i == 10 {
        assert!(rate_limiter.check().is_err());
      } else {
        assert!(rate_limiter.check().is_ok());
      }
    }
    assert!(rate_limiter.check().is_err());
    assert!(rate_limiter.check().is_err());

    sleep(Duration::from_secs(1)).await;
    for i in 0..=10 {
      if i == 10 {
        assert!(rate_limiter.check().is_err());
      } else {
        assert!(rate_limiter.check().is_ok());
      }
    }
  }
}
