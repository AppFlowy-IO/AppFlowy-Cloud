use std::ops::Deref;

use actix::{Actor, Context, Handler, ResponseFuture};
use tracing::{error, info, warn};

use crate::error::RealtimeError;
use crate::CollaborationServer;
use access_control::collab::RealtimeAccessControl;
use collab_rt_entity::user::UserDevice;
use database::collab::CollabStorage;

use crate::actix_ws::client::rt_client::{RealtimeClientWebsocketSinkImpl, RealtimeServer};
use crate::actix_ws::entities::{ClientMessage, ClientStreamMessage, Connect, Disconnect};

#[derive(Clone)]
pub struct RealtimeServerActor<S, AC>(pub CollaborationServer<S, AC>);

impl<S, AC> RealtimeServer for RealtimeServerActor<S, AC>
where
  S: CollabStorage + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
}

impl<S, AC> Deref for RealtimeServerActor<S, AC> {
  type Target = CollaborationServer<S, AC>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl<S, AC> Actor for RealtimeServerActor<S, AC>
where
  S: 'static + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
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
impl<S, AC> actix::Supervised for RealtimeServerActor<S, AC>
where
  S: 'static + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  fn restarting(&mut self, ctx: &mut Context<RealtimeServerActor<S, AC>>) {
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

impl<S, AC> Handler<Connect> for RealtimeServerActor<S, AC>
where
  S: CollabStorage + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  type Result = ResponseFuture<anyhow::Result<(), RealtimeError>>;

  fn handle(&mut self, new_conn: Connect, _ctx: &mut Context<Self>) -> Self::Result {
    let conn_sink = RealtimeClientWebsocketSinkImpl(new_conn.socket);
    self.handle_new_connection(new_conn.user, conn_sink)
  }
}

impl<S, AC> Handler<Disconnect> for RealtimeServerActor<S, AC>
where
  S: CollabStorage + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  type Result = ResponseFuture<anyhow::Result<(), RealtimeError>>;
  fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) -> Self::Result {
    self.handle_disconnect(msg.user)
  }
}

impl<S, AC> Handler<ClientMessage> for RealtimeServerActor<S, AC>
where
  S: CollabStorage + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  type Result = ResponseFuture<anyhow::Result<(), RealtimeError>>;

  fn handle(&mut self, client_msg: ClientMessage, _ctx: &mut Context<Self>) -> Self::Result {
    let ClientMessage { user, message } = client_msg;
    match message.transform() {
      Ok(message_by_object_id) => self.handle_client_message(user, message_by_object_id),
      Err(err) => {
        if cfg!(debug_assertions) {
          error!("parse client message error: {}", err);
        }
        Box::pin(async { Ok(()) })
      },
    }
  }
}

impl<S, AC> Handler<ClientStreamMessage> for RealtimeServerActor<S, AC>
where
  S: CollabStorage + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  type Result = ResponseFuture<anyhow::Result<(), RealtimeError>>;

  fn handle(&mut self, client_msg: ClientStreamMessage, _ctx: &mut Context<Self>) -> Self::Result {
    let ClientStreamMessage {
      uid,
      device_id,
      message,
    } = client_msg;

    // Get the real-time user by the device ID and user ID. If the user is not found, which means
    // the user is not connected to the real-time server via websocket.
    let user = self.get_user_by_device(&UserDevice::new(&device_id, uid));
    match (user, message.transform()) {
      (Some(user), Ok(messages)) => self.handle_client_message(user, messages),
      (None, _) => {
        warn!("Can't find the realtime user uid:{}, device:{}. User should connect via websocket before", uid,device_id);
        Box::pin(async { Ok(()) })
      },
      (Some(_), Err(err)) => {
        if cfg!(debug_assertions) {
          error!("parse client message error: {}", err);
        }
        Box::pin(async { Ok(()) })
      },
    }
  }
}
