use crate::error::WSError;
use actix::{Message, Recipient};
use bytes::Bytes;
use collab_plugins::sync::msg::CollabMessage;
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct WSUser {
  pub user_id: Secret<String>,
}

impl Display for WSUser {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_str(self.user_id.expose_secret())
  }
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
  pub business_id: String,
  pub user: Arc<WSUser>,
  pub collab_msg: CollabMessage,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "()")]
pub struct ServerMessage {
  pub business_id: String,
  pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WSMessage {
  pub business_id: String,
  pub payload: Vec<u8>,
}

impl WSMessage {
  pub fn from_vec(bytes: Vec<u8>) -> Result<Self, serde_json::Error> {
    serde_json::from_slice(&bytes)
  }
}

impl From<WSMessage> for Bytes {
  fn from(msg: WSMessage) -> Self {
    let bytes = serde_json::to_vec(&msg).unwrap_or_default();
    Bytes::from(bytes)
  }
}

impl From<ServerMessage> for WSMessage {
  fn from(server_msg: ServerMessage) -> Self {
    Self {
      business_id: server_msg.business_id,
      payload: server_msg.payload,
    }
  }
}

impl From<CollabMessage> for WSMessage {
  fn from(msg: CollabMessage) -> Self {
    Self {
      business_id: msg.business_id().to_string(),
      payload: msg.to_vec(),
    }
  }
}

impl From<CollabMessage> for ServerMessage {
  fn from(msg: CollabMessage) -> Self {
    Self {
      business_id: msg.business_id().to_string(),
      payload: msg.to_vec(),
    }
  }
}

impl From<ClientMessage> for WSMessage {
  fn from(client_msg: ClientMessage) -> Self {
    Self {
      business_id: client_msg.business_id,
      payload: client_msg.collab_msg.to_vec(),
    }
  }
}
