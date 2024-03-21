use crate::server::{RealtimeAccessControl, RealtimeServer};
use actix::{Actor, Context};
use realtime_entity::user::RealtimeUser;

impl<S, U, AC> Actor for RealtimeServer<S, U, AC>
where
  S: 'static + Unpin,
  U: RealtimeUser + Unpin,
  AC: RealtimeAccessControl + Unpin,
{
  type Context = Context<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    ctx.set_mailbox_capacity(3000);
  }
}
