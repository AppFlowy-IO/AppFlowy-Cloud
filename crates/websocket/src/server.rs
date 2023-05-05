use crate::entities::{
  Connect, Disconnect, ServerBroadcastMessage, ServerMessage, WebSocketMessage,
};
use crate::error::WSError;
use actix::{Actor, Context, Handler, Recipient};
use collab_sync::server::CollabGroup;
use std::collections::HashMap;
use std::sync::Arc;

pub type CollabId = String;

pub struct CollabServer {
  collab_group_map: HashMap<CollabId, CollabGroup>,
  sessions: HashMap<usize, Recipient<ServerMessage>>,
}

impl CollabServer {
  pub fn new(collab_group_map: HashMap<String, CollabGroup>) -> Self {
    Self {
      collab_group_map,
      sessions: Default::default(),
    }
  }

  pub fn broadcast_message(&self, msg: ServerBroadcastMessage) {
    // self.collab_group_map.get(&msg.collab_id).map(|collab_group| {
    //   collab_group.broadcast_message(msg);
    // }
  }
}

impl Actor for CollabServer {
  type Context = Context<Self>;
}

/// New collab connection
impl Handler<Connect> for CollabServer {
  type Result = Result<(), WSError>;
  fn handle(&mut self, msg: Connect, _ctx: &mut Context<Self>) -> Self::Result {
    let broadcast_msg = ServerBroadcastMessage {
      collab_id: msg.collab_id.clone(),
    };
    self.broadcast_message(broadcast_msg);
    Ok(())
  }
}

impl Handler<Disconnect> for CollabServer {
  type Result = Result<(), WSError>;
  fn handle(&mut self, _msg: Disconnect, _: &mut Context<Self>) -> Self::Result {
    Ok(())
  }
}

impl Handler<WebSocketMessage> for CollabServer {
  type Result = ();

  fn handle(&mut self, _msg: WebSocketMessage, _ctx: &mut Context<Self>) -> Self::Result {}
}

impl actix::Supervised for CollabServer {
  fn restarting(&mut self, _ctx: &mut Context<CollabServer>) {
    tracing::warn!("restarting");
  }
}
