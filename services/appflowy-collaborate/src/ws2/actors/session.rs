use super::server::{Join, Leave, WsOutput, WsServer};
use actix::{
  fut, Actor, ActorContext, ActorFutureExt, Addr, AsyncContext, ContextFutureSpawner, Handler,
  Running, StreamHandler, WrapFuture,
};
use actix_http::ws::{CloseCode, CloseReason, Item, ProtocolError};
use actix_web_actors::ws;
use appflowy_proto::{ClientMessage, ObjectId, Rid, ServerMessage, UpdateFlags, WorkspaceId};
use bytes::{Bytes, BytesMut};
use collab::core::origin::{CollabClient, CollabOrigin};
use collab_entity::CollabType;
use collab_stream::model::MessageId;
use std::time::{Duration, Instant};
use tracing::error;
use yrs::block::ClientID;
use yrs::sync::AwarenessUpdate;
use yrs::updates::decoder::Decode;
use yrs::StateVector;

pub const HEARTBEAT: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub struct SessionInfo {
  pub client_id: ClientID,
  pub user_id: i64,
  pub device_id: String,
  pub last_message_id: Option<MessageId>,
}

impl SessionInfo {
  pub fn new(
    client_id: ClientID,
    user_id: i64,
    device_id: String,
    last_message_id: Option<MessageId>,
  ) -> Self {
    Self {
      client_id,
      user_id,
      device_id,
      last_message_id,
    }
  }

  pub fn collab_origin(&self) -> CollabOrigin {
    CollabOrigin::Client(CollabClient::new(self.user_id, self.device_id.clone()))
  }
}

pub type ExtraMessageReceiver = tokio::sync::mpsc::Receiver<ServerMessage>;
pub struct WsSession {
  current_workspace: WorkspaceId,
  info: SessionInfo,
  server: Addr<WsServer>,
  hb: Instant,
  buf: Option<BytesMut>,
  extra_message_rx: Option<ExtraMessageReceiver>,
}

impl WsSession {
  pub fn new(
    workspace: WorkspaceId,
    info: SessionInfo,
    server: Addr<WsServer>,
    extra_message_rx: ExtraMessageReceiver,
  ) -> Self {
    WsSession {
      info,
      server,
      current_workspace: workspace,
      hb: Instant::now(),
      buf: None,
      extra_message_rx: Some(extra_message_rx),
    }
  }

  pub fn uid(&self) -> i64 {
    self.info.user_id
  }

  /// Unique identifier of current session.
  fn id(&self) -> ClientID {
    self.info.client_id
  }

  fn hb(&self, ctx: &mut ws::WebsocketContext<Self>) {
    ctx.run_interval(HEARTBEAT, |act, ctx| {
      if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
        tracing::trace!(
          "session `{}` failed to receive pong within {:?}",
          act.id(),
          CLIENT_TIMEOUT
        );
        act.server.do_send(Leave {
          session_id: act.id(),
          workspace_id: act.current_workspace,
        });
        ctx.stop();
        return;
      }
      ctx.ping(b"");
    });
  }

  fn handle_protocol(&mut self, bytes: Bytes, ctx: &mut ws::WebsocketContext<Self>) {
    match ClientMessage::from_bytes(&bytes) {
      Ok(message) => {
        tracing::trace!(
          "received message from session `{}`: {:#?}",
          self.id(),
          message
        );
        let object_id = *message.object_id();
        let message = match InputMessage::try_from(message) {
          Ok(msg) => msg,
          Err(err) => return ctx.close(Some(CloseReason::from((CloseCode::Invalid, err)))),
        };
        self.server.do_send(WsInput {
          message,
          workspace_id: self.current_workspace,
          object_id,
          client_id: self.id(),
          sender: self.info.collab_origin(),
        });
      },
      Err(err) => {
        tracing::warn!("client {} send invalid message: {}", self.id(), err);
        ctx.close(Some(CloseReason::from((
          CloseCode::Invalid,
          err.to_string(),
        ))));
      },
    }
  }
}

impl Actor for WsSession {
  type Context = ws::WebsocketContext<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    tracing::trace!("starting session `{}`", self.id());
    self.hb(ctx);

    let recipient = ctx.address().recipient();
    if let Some(mut external_source) = self.extra_message_rx.take() {
      actix::spawn(async move {
        while let Some(message) = external_source.recv().await {
          let output = WsOutput { message };
          let _ = recipient.send(output).await;
        }
      });
    } else {
      error!("extra_message_sender only take once");
    }

    let join = Join {
      uid: self.info.user_id,
      session_id: self.id(),
      collab_origin: self.info.collab_origin(),
      addr: ctx.address(),
      last_message_id: self.info.last_message_id.map(MessageId::into),
      workspace_id: self.current_workspace,
    };
    self
      .server
      .send(join)
      .into_actor(self)
      .then(|res, act, ctx| {
        if let Err(err) = res {
          tracing::warn!("session `{}` can't join: {}", act.id(), err);
          ctx.stop();
        }
        fut::ready(())
      })
      .wait(ctx);
  }

  fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
    tracing::trace!("stopping session `{}`", self.id());
    self.server.do_send(Leave {
      session_id: self.id(),
      workspace_id: self.current_workspace,
    });
    Running::Stop
  }
}

impl Handler<WsOutput> for WsSession {
  type Result = ();

  fn handle(&mut self, msg: WsOutput, ctx: &mut Self::Context) {
    tracing::trace!(
      "sending message through session `{}`: {:#?}",
      self.id(),
      msg.message
    );
    if let Ok(bytes) = msg.message.into_bytes() {
      ctx.binary(bytes);
    };
  }
}

impl Handler<GetUid> for WsSession {
  type Result = i64;

  fn handle(&mut self, _msg: GetUid, _ctx: &mut Self::Context) -> Self::Result {
    self.uid()
  }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
  fn handle(&mut self, item: Result<ws::Message, ProtocolError>, ctx: &mut Self::Context) {
    match item {
      Ok(message) => {
        self.hb = Instant::now();
        match message {
          ws::Message::Ping(bytes) => {
            ctx.pong(&bytes);
          },
          ws::Message::Pong(_) => {},
          ws::Message::Text(data) => ctx.text(data), // echo text messages back
          ws::Message::Binary(bytes) => {
            self.handle_protocol(bytes, ctx);
          },
          ws::Message::Continuation(item) => {
            match item {
              Item::FirstText(_) => ctx.close(Some(CloseReason::from((
                CloseCode::Unsupported,
                "only binary content is supported",
              )))),
              Item::FirstBinary(bytes) => self.buf = Some(bytes.into()),
              Item::Continue(bytes) => {
                if let Some(ref mut buf) = self.buf {
                  buf.extend_from_slice(&bytes);
                } else {
                  ctx.close(Some(CloseReason::from((
                    CloseCode::Protocol,
                    "continuation frame without initialization",
                  )))); // unexpected
                }
              },
              Item::Last(bytes) => {
                if let Some(mut buf) = self.buf.take() {
                  buf.extend_from_slice(&bytes);
                  self.handle_protocol(buf.freeze(), ctx);
                } else {
                  ctx.close(Some(CloseReason::from((
                    CloseCode::Protocol,
                    "last frame without initialization",
                  )))); // unexpected
                }
              },
            }
          },
          ws::Message::Close(reason) => {
            tracing::trace!("session `{}` closed by the client: {:?}", self.id(), reason);
            ctx.close(reason);
            ctx.stop();
          },
          _ => { /* do nothing */ },
        }
      },
      Err(err) => {
        tracing::warn!("session `{}` websocket protocol error: {}", self.id(), err);
        ctx.stop();
      },
    }
  }
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct WsInput {
  pub message: InputMessage,
  pub workspace_id: ObjectId,
  pub object_id: ObjectId,
  pub sender: CollabOrigin,
  pub client_id: ClientID,
}

#[derive(actix::Message)]
#[rtype(result = "i64")]
pub struct GetUid;

pub enum InputMessage {
  Manifest(CollabType, Rid, StateVector),
  Update(CollabType, UpdateFlags, Vec<u8>),
  AwarenessUpdate(AwarenessUpdate),
}

impl TryFrom<ClientMessage> for InputMessage {
  type Error = String;

  fn try_from(value: ClientMessage) -> Result<Self, Self::Error> {
    match value {
      ClientMessage::Manifest {
        last_message_id,
        state_vector,
        collab_type,
        ..
      } => {
        let state_vector = StateVector::decode_v1(&state_vector).map_err(|err| err.to_string())?;
        Ok(InputMessage::Manifest(
          collab_type,
          last_message_id,
          state_vector,
        ))
      },
      ClientMessage::Update {
        flags,
        update,
        collab_type,
        ..
      } => Ok(InputMessage::Update(collab_type, flags, update)),
      ClientMessage::AwarenessUpdate { awareness, .. } => {
        let awareness = AwarenessUpdate::decode_v1(&awareness).map_err(|err| err.to_string())?;
        Ok(InputMessage::AwarenessUpdate(awareness))
      },
    }
  }
}
