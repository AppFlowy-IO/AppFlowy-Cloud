use actix::{Message, Recipient};
use collab_rt::error::RealtimeError;

use serde_repr::{Deserialize_repr, Serialize_repr};
use std::fmt::Debug;

use collab_rt_entity::user::RealtimeUser;
pub use collab_rt_entity::RealtimeMessage;

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Connect {
  pub socket: Recipient<RealtimeMessage>,
  pub user: RealtimeUser,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Disconnect {
  pub user: RealtimeUser,
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
pub struct ClientMessage {
  pub user: RealtimeUser,
  pub message: RealtimeMessage,
}

#[derive(Message)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct ClientStreamMessage {
  pub uid: i64,
  pub device_id: String,
  pub message: RealtimeMessage,
}
