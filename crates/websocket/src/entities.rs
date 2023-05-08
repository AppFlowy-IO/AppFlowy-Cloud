use crate::error::WSError;
use actix::{Message, Recipient};
use collab_plugins::sync::msg::CollabMessage;
use secrecy::{ExposeSecret, Secret};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct WSUser {
  pub user_id: Secret<String>,
}

impl Hash for WSUser {
  fn hash<H: Hasher>(&self, state: &mut H) {
    let uid: &String = self.user_id.expose_secret();
    uid.hash(state);
  }
}

impl PartialEq<Self> for WSUser {
  fn eq(&self, other: &Self) -> bool {
    let uid: &String = self.user_id.expose_secret();
    let other_uid: &String = other.user_id.expose_secret();
    uid == other_uid
  }
}

impl Eq for WSUser {}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), WSError>")]
pub struct Connect {
  pub socket: Recipient<ServerMessage>,
  pub user: Arc<WSUser>,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), WSError>")]
pub struct Disconnect {
  pub user: Arc<WSUser>,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "()")]
pub struct ClientMessage {
  pub user: Arc<WSUser>,
  pub collab_msg: CollabMessage,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "()")]
pub struct ServerMessage {
  pub collab_msg: CollabMessage,
}
