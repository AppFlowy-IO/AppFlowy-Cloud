use std::ops::Deref;

use crate::error::RealtimeError;
use crate::CollaborationServer;
use actix::{Actor, Context, Handler};
use anyhow::anyhow;
use app_error::AppError;
use collab_rt_entity::user::UserDevice;
use tracing::{error, info, trace, warn};

use crate::actix_ws::client::rt_client::{RealtimeClientWebsocketSinkImpl, RealtimeServer};
use crate::actix_ws::entities::{
  ClientGenerateEmbeddingMessage, ClientHttpStreamMessage, ClientHttpUpdateMessage,
  ClientWebSocketMessage, Connect, Disconnect,
};

#[derive(Clone)]
pub struct RealtimeServerActor(pub CollaborationServer);

impl RealtimeServer for RealtimeServerActor {}

impl Deref for RealtimeServerActor {
  type Target = CollaborationServer;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl Actor for RealtimeServerActor {
  type Context = Context<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    let mail_box_size = mail_box_size();
    info!(
      "realtime server started with mailbox size: {}",
      mail_box_size
    );
    ctx.set_mailbox_capacity(mail_box_size);
  }
}
impl actix::Supervised for RealtimeServerActor {
  fn restarting(&mut self, ctx: &mut Context<RealtimeServerActor>) {
    error!("realtime server is restarting");
    ctx.set_mailbox_capacity(mail_box_size());
  }
}

fn mail_box_size() -> usize {
  match std::env::var("APPFLOWY_WEBSOCKET_MAILBOX_SIZE") {
    Ok(value) => value.parse::<usize>().unwrap_or_else(|_| {
      error!("Error: Invalid mailbox size format, defaulting to 6000");
      6000
    }),
    Err(_) => 6000,
  }
}

impl Handler<Connect> for RealtimeServerActor {
  type Result = anyhow::Result<(), RealtimeError>;

  fn handle(&mut self, new_conn: Connect, _ctx: &mut Context<Self>) -> Self::Result {
    let conn_sink = RealtimeClientWebsocketSinkImpl(new_conn.socket);
    trace!(
      "New connection from user: {}, device: {}",
      new_conn.user.uid,
      new_conn.user.device_id
    );
    self.handle_new_connection(new_conn.user, conn_sink)
  }
}

impl Handler<Disconnect> for RealtimeServerActor {
  type Result = anyhow::Result<(), RealtimeError>;
  fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) -> Self::Result {
    self.handle_disconnect(msg.user)
  }
}

impl Handler<ClientWebSocketMessage> for RealtimeServerActor {
  type Result = anyhow::Result<(), RealtimeError>;

  fn handle(
    &mut self,
    client_msg: ClientWebSocketMessage,
    _ctx: &mut Context<Self>,
  ) -> Self::Result {
    let ClientWebSocketMessage { user, message } = client_msg;
    match message.split_messages_by_object_id() {
      Ok(message_by_object_id) => self.handle_client_message(user, message_by_object_id),
      Err(err) => {
        if cfg!(debug_assertions) {
          error!("parse client message error: {}", err);
        }
        Ok(())
      },
    }
  }
}

impl Handler<ClientHttpStreamMessage> for RealtimeServerActor {
  type Result = anyhow::Result<(), RealtimeError>;

  fn handle(
    &mut self,
    client_msg: ClientHttpStreamMessage,
    _ctx: &mut Context<Self>,
  ) -> Self::Result {
    let ClientHttpStreamMessage {
      uid,
      device_id,
      message,
    } = client_msg;

    // Get the real-time user by the device ID and user ID. If the user is not found, which means
    // the user is not connected to the real-time server via websocket.
    let user = self.get_user_by_device(&UserDevice::new(&device_id, uid));
    match (user, message.split_messages_by_object_id()) {
      (Some(user), Ok(messages)) => self.handle_client_message(user, messages),
      (None, _) => {
        warn!("Can't find the realtime user uid:{}, device:{}. User should connect via websocket before", uid,device_id);
        Ok(())
      },
      (Some(_), Err(err)) => {
        if cfg!(debug_assertions) {
          error!("parse client message error: {}", err);
        }
        Ok(())
      },
    }
  }
}

impl Handler<ClientHttpUpdateMessage> for RealtimeServerActor {
  type Result = Result<(), AppError>;

  fn handle(&mut self, msg: ClientHttpUpdateMessage, _ctx: &mut Self::Context) -> Self::Result {
    trace!("Receive client http update message");
    self
      .handle_client_http_update(msg)
      .map_err(|err| AppError::Internal(anyhow!("handle client http message error: {}", err)))?;
    Ok(())
  }
}

impl Handler<ClientGenerateEmbeddingMessage> for RealtimeServerActor {
  type Result = Result<(), AppError>;

  fn handle(
    &mut self,
    msg: ClientGenerateEmbeddingMessage,
    _ctx: &mut Self::Context,
  ) -> Self::Result {
    self
      .handle_client_generate_embedding_request(msg)
      .map_err(|err| {
        AppError::Internal(anyhow!(
          "handle client generate embedding request error: {}",
          err
        ))
      })?;
    Ok(())
  }
}
