use super::server::{Join, Leave, WsOutput, WsServer};
use actix::{
  fut, Actor, ActorContext, ActorFutureExt, Addr, AsyncContext, ContextFutureSpawner, Handler,
  Running, StreamHandler, WrapFuture,
};
use actix_http::ws::{CloseCode, CloseReason, Item, ProtocolError};
use actix_web_actors::ws;
use appflowy_proto::{ClientMessage, ObjectId, Rid, UpdateFlags, WorkspaceId};
use bytes::{Bytes, BytesMut};
use collab::core::origin::{CollabClient, CollabOrigin};
use collab_entity::CollabType;
use std::time::{Duration, Instant};
use yrs::block::ClientID;
use yrs::sync::AwarenessUpdate;
use yrs::updates::decoder::Decode;
use yrs::{StateVector, Update};

pub const HEARTBEAT: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub struct SessionInfo {
  pub client_id: ClientID,
  pub user_id: i64,
  pub device_id: String,
}

impl SessionInfo {
  pub fn new(client_id: ClientID, user_id: i64, device_id: String) -> Self {
    Self {
      client_id,
      user_id,
      device_id,
    }
  }

  pub fn collab_origin(&self) -> CollabOrigin {
    CollabOrigin::Client(CollabClient::new(self.user_id, self.device_id.clone()))
  }
}

pub struct WsSession {
  current_workspace: WorkspaceId,
  info: SessionInfo,
  server: Addr<WsServer>,
  hb: Instant,
  buf: Option<BytesMut>,
}

impl WsSession {
  pub fn new(workspace: WorkspaceId, info: SessionInfo, server: Addr<WsServer>) -> Self {
    WsSession {
      info,
      server,
      current_workspace: workspace,
      hb: Instant::now(),
      buf: None,
    }
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
    tracing::trace!("session `{}` broadcasting {} bytes", self.id(), bytes.len());
    match ClientMessage::from_bytes(&bytes) {
      Ok(message) => {
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
    let join = Join {
      session_id: self.id(),
      collab_origin: self.info.collab_origin(),
      addr: ctx.address(),
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
    if let Ok(bytes) = msg.message.into_bytes() {
      tracing::trace!(
        "sending message through session `{}`: {} bytes",
        self.id(),
        bytes.len()
      );
      ctx.binary(bytes);
    };
  }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
  fn handle(&mut self, item: Result<ws::Message, ProtocolError>, ctx: &mut Self::Context) {
    match item {
      Ok(message) => {
        tracing::trace!("received message: {:?}", message);
        self.hb = Instant::now();
        match message {
          ws::Message::Ping(bytes) => {
            ctx.pong(&bytes);
          },
          ws::Message::Pong(_) => {},
          ws::Message::Text(_) => ctx.close(Some(CloseReason::from((
            CloseCode::Unsupported,
            "only binary content is supported",
          )))),
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

pub enum InputMessage {
  Manifest(CollabType, Rid, StateVector),
  Update(CollabType, Update),
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
      } => {
        let update = match flags {
          UpdateFlags::Lib0v1 => Update::decode_v1(&update),
          UpdateFlags::Lib0v2 => Update::decode_v2(&update),
        }
        .map_err(|err| err.to_string())?;
        Ok(InputMessage::Update(collab_type, update))
      },
      ClientMessage::AwarenessUpdate { awareness, .. } => {
        let awareness = AwarenessUpdate::decode_v1(&awareness).map_err(|err| err.to_string())?;
        Ok(InputMessage::AwarenessUpdate(awareness))
      },
    }
  }
}
