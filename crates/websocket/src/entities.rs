use crate::error::WSError;
use actix::{Message, Recipient};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::fmt::Formatter;
use std::sync::Arc;

pub type Socket = Recipient<WebSocketMessage>;

#[derive(Debug)]
pub struct LoggedUser();

pub struct Session {
  pub user: Arc<LoggedUser>,
  pub socket: Socket,
}

impl std::convert::From<Connect> for Session {
  fn from(c: Connect) -> Self {
    Self {
      user: c.user,
      socket: c.socket,
    }
  }
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), WSError>")]
pub struct Connect {
  pub collab_id: String,
  pub socket: Socket,
  pub user: Arc<LoggedUser>,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), WSError>")]
pub struct Disconnect {
  pub user: Arc<LoggedUser>,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "()")]
pub struct WebSocketMessage(pub Bytes);

impl std::ops::Deref for WebSocketMessage {
  type Target = Bytes;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct ServerMessage(pub String);

#[derive(Debug, Message, Clone)]
#[rtype(result = "()")]
pub struct ServerBroadcastMessage {
  pub collab_id: String,
}
