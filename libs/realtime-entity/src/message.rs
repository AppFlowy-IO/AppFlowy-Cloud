use crate::collab_msg::CollabMessage;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use websocket::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
  feature = "actix_message",
  derive(actix::Message),
  rtype(result = "()")
)]
pub enum RealtimeMessage {
  Collab(CollabMessage),
  User(UserMessage),
  System(SystemMessage),
}

impl RealtimeMessage {
  pub fn device_id(&self) -> Option<String> {
    match self {
      RealtimeMessage::Collab(msg) => msg.device_id(),
      RealtimeMessage::User(_) => None,
      RealtimeMessage::System(_) => None,
    }
  }
}

impl Display for RealtimeMessage {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      RealtimeMessage::Collab(msg) => f.write_fmt(format_args!("Collab:{}", msg.object_id())),
      RealtimeMessage::User(_) => f.write_fmt(format_args!("User")),
      RealtimeMessage::System(_) => f.write_fmt(format_args!("System")),
    }
  }
}

impl From<RealtimeMessage> for Bytes {
  fn from(msg: RealtimeMessage) -> Self {
    let bytes = bincode::serialize(&msg).unwrap_or_default();
    Bytes::from(bytes)
  }
}

impl From<RealtimeMessage> for Vec<u8> {
  fn from(msg: RealtimeMessage) -> Self {
    bincode::serialize(&msg).unwrap_or_default()
  }
}

impl TryFrom<Bytes> for RealtimeMessage {
  type Error = bincode::Error;

  fn try_from(value: Bytes) -> Result<Self, Self::Error> {
    bincode::deserialize(&value)
  }
}

impl TryFrom<&[u8]> for RealtimeMessage {
  type Error = bincode::Error;

  fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
    bincode::deserialize(value)
  }
}

impl TryFrom<&Vec<u8>> for RealtimeMessage {
  type Error = bincode::Error;

  fn try_from(value: &Vec<u8>) -> Result<Self, Self::Error> {
    bincode::deserialize(value)
  }
}

impl TryFrom<Vec<u8>> for RealtimeMessage {
  type Error = bincode::Error;

  fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
    bincode::deserialize(&value)
  }
}

use crate::user::UserMessage;

impl TryFrom<&Message> for RealtimeMessage {
  type Error = anyhow::Error;

  fn try_from(value: &Message) -> Result<Self, Self::Error> {
    match value {
      Message::Binary(bytes) => RealtimeMessage::try_from(bytes).map_err(anyhow::Error::from),
      _ => Err(anyhow::anyhow!("Unsupported message type")),
    }
  }
}

impl TryFrom<Message> for RealtimeMessage {
  type Error = anyhow::Error;

  fn try_from(value: Message) -> Result<Self, Self::Error> {
    match value {
      Message::Binary(bytes) => RealtimeMessage::try_from(bytes).map_err(anyhow::Error::from),
      _ => Err(anyhow::anyhow!("Unsupported message type")),
    }
  }
}

impl From<RealtimeMessage> for Message {
  fn from(msg: RealtimeMessage) -> Self {
    let bytes = bincode::serialize(&msg).unwrap_or_default();
    Message::Binary(bytes)
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemMessage {
  RateLimit(u32),
  KickOff,
}
