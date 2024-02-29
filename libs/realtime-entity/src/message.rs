use crate::collab_msg::{ClientCollabMessage, CollabMessage, ServerCollabMessage};
use anyhow::{anyhow, Error};
use bincode::{DefaultOptions, Options};
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
  ClientCollabV1(Vec<ClientCollabMessage>),
  ServerCollabV1(Vec<ServerCollabMessage>),
}

impl RealtimeMessage {
  pub fn size(&self) -> usize {
    match self {
      RealtimeMessage::Collab(msg) => msg.len(),
      RealtimeMessage::User(_) => 1,
      RealtimeMessage::System(_) => 1,
      RealtimeMessage::ClientCollabV1(msgs) => msgs.iter().map(|msg| msg.size()).sum(),
      RealtimeMessage::ServerCollabV1(msgs) => msgs.iter().map(|msg| msg.size()).sum(),
    }
  }

  /// Convert RealtimeMessage to ClientCollabMessage
  /// If the message is not a collab message, it will return an empty vec
  /// If the message is a collab message, it will return a vec with one element
  /// If the message is a ClientCollabV1, it will return list of collab messages
  pub fn try_into_client_collab_message(self) -> Result<Vec<ClientCollabMessage>, Error> {
    match self {
      RealtimeMessage::Collab(collab_message) => {
        let collab_message = ClientCollabMessage::try_from(collab_message)?;
        Ok(vec![collab_message])
      },
      RealtimeMessage::ClientCollabV1(collab_messages) => Ok(collab_messages),
      _ => Err(anyhow!(
        "Failed to convert RealtimeMessage:{} to ClientCollabMessage",
        self
      )),
    }
  }
}

impl Display for RealtimeMessage {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      RealtimeMessage::Collab(msg) => f.write_fmt(format_args!("Collab:{}", msg)),
      RealtimeMessage::User(_) => f.write_fmt(format_args!("User")),
      RealtimeMessage::System(_) => f.write_fmt(format_args!("System")),
      RealtimeMessage::ClientCollabV1(_) => f.write_fmt(format_args!("ClientCollabV1")),
      RealtimeMessage::ServerCollabV1(_) => f.write_fmt(format_args!("ServerCollabV1")),
    }
  }
}

impl From<RealtimeMessage> for Bytes {
  fn from(msg: RealtimeMessage) -> Self {
    let data: Vec<u8> = msg.into();
    Bytes::from(data)
  }
}

impl From<RealtimeMessage> for Vec<u8> {
  fn from(msg: RealtimeMessage) -> Self {
    DefaultOptions::new()
      .with_fixint_encoding()
      .allow_trailing_bytes()
        .with_limit( 2 * 1024 * 1024) // 2 MB
      .serialize(&msg)
      .unwrap_or_default()
  }
}

impl TryFrom<&Vec<u8>> for RealtimeMessage {
  type Error = bincode::Error;

  fn try_from(value: &Vec<u8>) -> Result<Self, Self::Error> {
    Self::try_from(value.as_slice())
  }
}

impl TryFrom<&[u8]> for RealtimeMessage {
  type Error = bincode::Error;

  fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
    DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit( 2 * 1024 * 1024) // 2 MB
        .deserialize(value)
  }
}

use crate::user::UserMessage;

impl TryFrom<&Message> for RealtimeMessage {
  type Error = anyhow::Error;

  fn try_from(value: &Message) -> Result<Self, Self::Error> {
    match value {
      Message::Binary(bytes) => {
        RealtimeMessage::try_from(bytes.as_slice()).map_err(anyhow::Error::from)
      },
      _ => Err(anyhow::anyhow!("Unsupported message type")),
    }
  }
}

impl TryFrom<Message> for RealtimeMessage {
  type Error = anyhow::Error;

  fn try_from(value: Message) -> Result<Self, Self::Error> {
    match value {
      Message::Binary(bytes) => {
        RealtimeMessage::try_from(bytes.as_slice()).map_err(anyhow::Error::from)
      },
      _ => Err(anyhow::anyhow!("Unsupported message type")),
    }
  }
}

impl From<RealtimeMessage> for Message {
  fn from(msg: RealtimeMessage) -> Self {
    let data: Vec<u8> = msg.into();
    Message::Binary(data)
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemMessage {
  RateLimit(u32),
  KickOff,
}

#[cfg(test)]
mod tests {

  use crate::collab_msg::{CollabMessage, InitSync};
  use crate::message::{RealtimeMessage, SystemMessage};
  use crate::user::UserMessage;
  use collab::core::origin::CollabOrigin;
  use collab_entity::CollabType;
  use serde::{Deserialize, Serialize};

  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[cfg_attr(
    feature = "actix_message",
    derive(actix::Message),
    rtype(result = "()")
  )]
  pub enum RealtimeMessageV1 {
    Collab(CollabMessage),
    User(UserMessage),
    System(SystemMessage),
  }

  #[test]
  fn decode_version_1_with_version_2_struct_test() {
    let version_1 = RealtimeMessageV1::Collab(CollabMessage::ClientInitSync(InitSync::new(
      CollabOrigin::Empty,
      "1".to_string(),
      CollabType::Document,
      "w1".to_string(),
      1,
      vec![0u8, 3],
    )));

    let version_1_bytes = bincode::serialize(&version_1).unwrap();
    let version_2 = RealtimeMessage::try_from(&version_1_bytes).unwrap();

    match (version_1, version_2) {
      (
        RealtimeMessageV1::Collab(CollabMessage::ClientInitSync(init_1)),
        RealtimeMessage::Collab(CollabMessage::ClientInitSync(init_2)),
      ) => {
        assert_eq!(init_1, init_2);
      },
      _ => panic!("Failed to convert RealtimeMessage to RealtimeMessage2"),
    }
  }

  #[test]
  fn decode_version_2_with_version_1_test_1() {
    let version_2 = RealtimeMessage::Collab(CollabMessage::ClientInitSync(InitSync::new(
      CollabOrigin::Empty,
      "1".to_string(),
      CollabType::Document,
      "w1".to_string(),
      1,
      vec![0u8, 3],
    )));

    let version_2_bytes: Vec<u8> = version_2.clone().into();
    let version_1: RealtimeMessageV1 = bincode::deserialize(&version_2_bytes).unwrap();

    match (version_1, version_2) {
      (
        RealtimeMessageV1::Collab(CollabMessage::ClientInitSync(init_1)),
        RealtimeMessage::Collab(CollabMessage::ClientInitSync(init_2)),
      ) => {
        assert_eq!(init_1, init_2);
      },
      _ => panic!("Failed to convert RealtimeMessage2 to RealtimeMessage"),
    }
  }
}
