use crate::error::{RealtimeError, StreamError};
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
  fn uid(&self) -> i64;
}

impl<T> RealtimeUser for Arc<T>
where
  T: RealtimeUser,
{
  fn uid(&self) -> i64 {
    self.as_ref().uid()
  }
}

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

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct DisconnectByServer;
#[derive(Debug, Clone, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum BusinessID {
  CollabId = 1,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct ClientMessage<U> {
  pub user: U,
  pub message: RealtimeMessage,
}

#[derive(Debug, Clone, Message, Serialize, Deserialize)]
#[rtype(result = "()")]
pub enum RealtimeMessage {
  Collab(CollabMessage),
  CloseClient,
}

impl From<RealtimeMessage> for Bytes {
  fn from(msg: RealtimeMessage) -> Self {
    let bytes = bincode::serialize(&msg).unwrap_or_default();
    Bytes::from(bytes)
  }
}

impl TryFrom<Bytes> for RealtimeMessage {
  type Error = bincode::Error;

  fn try_from(value: Bytes) -> Result<Self, Self::Error> {
    bincode::deserialize(&value)
  }
}

impl From<CollabMessage> for RealtimeMessage {
  fn from(msg: CollabMessage) -> Self {
    Self::Collab(msg)
  }
}

impl TryFrom<RealtimeMessage> for CollabMessage {
  type Error = StreamError;

  fn try_from(value: RealtimeMessage) -> Result<Self, Self::Error> {
    match value {
      RealtimeMessage::Collab(msg) => Ok(msg),
      _ => Err(StreamError::Internal("Invalid message type".to_string())),
    }
  }
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub(crate) struct Editing {
  pub object_id: String,
  pub origin: CollabOrigin,
}
