use crate::error::RealtimeError;
use actix::{Message, Recipient};
use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use collab_sync_protocol::CollabMessage;
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct RealtimeUser {
  pub user_id: Secret<String>,
}

impl Display for RealtimeUser {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_str(self.user_id.expose_secret())
  }
}

impl Hash for RealtimeUser {
  fn hash<H: Hasher>(&self, state: &mut H) {
    let uid: &String = self.user_id.expose_secret();
    uid.hash(state);
  }
}

impl PartialEq<Self> for RealtimeUser {
  fn eq(&self, other: &Self) -> bool {
    let uid: &String = self.user_id.expose_secret();
    let other_uid: &String = other.user_id.expose_secret();
    uid == other_uid
  }
}

impl Eq for RealtimeUser {}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Connect {
  pub socket: Recipient<ServerMessage>,
  pub user: Arc<RealtimeUser>,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Disconnect {
  pub user: Arc<RealtimeUser>,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct ClientMessage {
  pub business_id: u8,
  pub user: Arc<RealtimeUser>,
  pub content: CollabMessage,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "()")]
pub struct ServerMessage {
  pub business_id: u8,
  pub object_id: String,
  pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeMessage {
  pub business_id: u8,
  pub object_id: String,
  pub payload: Vec<u8>,
}

impl RealtimeMessage {
  pub fn from_vec(bytes: Vec<u8>) -> Result<Self, serde_json::Error> {
    serde_json::from_slice(&bytes)
  }
}

impl From<RealtimeMessage> for Bytes {
  fn from(msg: RealtimeMessage) -> Self {
    let bytes = serde_json::to_vec(&msg).unwrap_or_default();
    Bytes::from(bytes)
  }
}

impl From<ServerMessage> for RealtimeMessage {
  fn from(server_msg: ServerMessage) -> Self {
    Self {
      business_id: server_msg.business_id,
      object_id: server_msg.object_id,
      payload: server_msg.payload,
    }
  }
}

impl From<CollabMessage> for RealtimeMessage {
  fn from(msg: CollabMessage) -> Self {
    Self {
      business_id: msg.business_id(),
      object_id: msg.object_id().to_string(),
      payload: msg.to_vec(),
    }
  }
}

impl From<CollabMessage> for ServerMessage {
  fn from(msg: CollabMessage) -> Self {
    Self {
      business_id: msg.business_id(),
      object_id: msg.object_id().to_string(),
      payload: msg.to_vec(),
    }
  }
}

impl From<ClientMessage> for RealtimeMessage {
  fn from(client_msg: ClientMessage) -> Self {
    Self {
      business_id: client_msg.business_id,
      object_id: client_msg.content.object_id().to_string(),
      payload: client_msg.content.to_vec(),
    }
  }
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub(crate) struct EditCollab {
  pub object_id: String,
  pub origin: CollabOrigin,
}
