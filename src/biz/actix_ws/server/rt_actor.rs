use crate::biz::actix_ws::client::rt_client::RealtimeClientWebsocketSinkImpl;
use crate::biz::actix_ws::entities::{ClientMessage, ClientStreamMessage, Connect, Disconnect};
use actix::{Actor, Context, Handler, ResponseFuture};
use collab_rt::error::RealtimeError;
use collab_rt::{CollabRealtimeServer, RealtimeAccessControl};
use collab_rt_entity::user::UserDevice;
use database::collab::CollabStorage;
use std::ops::Deref;
use tracing::{error, warn};

#[derive(Clone)]
pub struct RealtimeServerActor<S, AC>(pub CollabRealtimeServer<S, AC>);

impl<S, AC> Deref for RealtimeServerActor<S, AC> {
  type Target = CollabRealtimeServer<S, AC>;

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
    ctx.set_mailbox_capacity(3000);
  }
}
impl<S, AC> actix::Supervised for RealtimeServerActor<S, AC>
where
  S: 'static + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  fn restarting(&mut self, _ctx: &mut Context<RealtimeServerActor<S, AC>>) {
    warn!("restarting");
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
    self.handle_new_connection(new_conn.user, new_conn.ws_connect_id, conn_sink)
  }
}

impl<S, AC> Handler<Disconnect> for RealtimeServerActor<S, AC>
where
  S: CollabStorage + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  type Result = ResponseFuture<anyhow::Result<(), RealtimeError>>;
  fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) -> Self::Result {
    self.handle_disconnect(msg.user, msg.ws_connect_id)
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

    let user = self
      .device_by_user
      .get(&UserDevice::new(&device_id, uid))
      .map(|entry| entry.value().clone());

    match (user, message.transform()) {
      (Some(user), Ok(messages)) => self.handle_client_message(user, messages),
      (None, _) => {
        warn!("user:{}|device:{} not found", uid, device_id);
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
