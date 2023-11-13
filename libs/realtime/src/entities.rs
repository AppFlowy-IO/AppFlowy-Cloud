use crate::error::RealtimeError;
use actix::{Message, Recipient};
use collab::core::origin::CollabOrigin;

use serde_repr::{Deserialize_repr, Serialize_repr};
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::sync::Arc;

pub use realtime_entity::message::RealtimeMessage;

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

#[derive(Debug, Hash, PartialEq, Eq)]
pub(crate) struct Editing {
  pub object_id: String,
  pub origin: CollabOrigin,
}
