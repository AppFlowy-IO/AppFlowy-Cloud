use crate::error::RealtimeError;
use actix::{Message, Recipient};
use collab::core::origin::CollabOrigin;

use serde_repr::{Deserialize_repr, Serialize_repr};
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::sync::Arc;
use tokio_stream::Stream;

pub use realtime_entity::message::RealtimeMessage;

pub trait RealtimeUser:
  Clone + Debug + Send + Sync + 'static + Display + Hash + Eq + PartialEq
{
  fn uid(&self) -> i64;
  fn device_id(&self) -> &str;
}

impl<T> RealtimeUser for Arc<T>
where
  T: RealtimeUser,
{
  fn uid(&self) -> i64 {
    self.as_ref().uid()
  }

  fn device_id(&self) -> &str {
    self.as_ref().device_id()
  }
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Connect<U> {
  pub socket: Recipient<RealtimeMessage>,
  pub user: U,
  pub session_id: String,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Disconnect<U> {
  pub user: U,
  pub session_id: String,
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

#[derive(Message)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct ClientStreamMessage {
  pub uid: i64,
  pub device_id: String,
  pub stream: Box<dyn Stream<Item = RealtimeMessage> + Unpin + Send>,
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub(crate) struct Editing {
  pub object_id: String,
  pub origin: CollabOrigin,
}
