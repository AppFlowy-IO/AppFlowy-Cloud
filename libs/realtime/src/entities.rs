use crate::error::RealtimeError;
use actix::{Message, Recipient};
use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use collab_sync_protocol::CollabMessage;

use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
use std::hash::Hash;

pub trait RealtimeUser: Clone + Debug + Send + Sync + 'static + Display {
  fn user_id(&self) -> &i64;
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Connect<U> {
  pub socket: Recipient<ServerMessage>,
  pub user: U,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Disconnect<U> {
  pub user: U,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct ClientMessage<U> {
  pub business_id: u8,
  pub user: U,
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

impl<U> From<ClientMessage<U>> for RealtimeMessage
where
  U: RealtimeUser,
{
  fn from(client_msg: ClientMessage<U>) -> Self {
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
