use crate::error::RealtimeError;
use actix::{Message, Recipient};
use bytes::Bytes;
use collab::core::origin::CollabOrigin;

use realtime_entity::collab_msg::CollabMessage;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::sync::Arc;

pub trait RealtimeUser:
  Clone + Debug + Send + Sync + 'static + Display + Hash + Eq + PartialEq
{
}

impl<T> RealtimeUser for Arc<T> where T: RealtimeUser {}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Connect<U> {
  pub socket: Recipient<RealtimeMessage>,
  pub user: U,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Disconnect<U> {
  pub user: U,
}

#[derive(Debug, Clone, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum BusinessID {
  CollabId = 1,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct ClientMessage<U> {
  pub business_id: BusinessID,
  pub user: U,
  pub content: CollabMessage,
}

#[derive(Debug, Clone, Message, Serialize, Deserialize)]
#[rtype(result = "()")]
pub struct RealtimeMessage {
  pub business_id: BusinessID,
  pub object_id: String,
  pub payload: Bytes,
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

impl From<CollabMessage> for RealtimeMessage {
  fn from(msg: CollabMessage) -> Self {
    Self {
      business_id: BusinessID::CollabId,
      object_id: msg.object_id().to_string(),
      payload: Bytes::from(msg.to_vec()),
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
      payload: Bytes::from(client_msg.content.to_vec()),
    }
  }
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub(crate) struct Editing {
  pub object_id: String,
  pub origin: CollabOrigin,
}
