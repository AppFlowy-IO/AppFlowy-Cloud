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
pub enum RealtimeMessageV1 {
  Collab(CollabMessage),
  User(UserMessage),
  System(SystemMessage),
}

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
  MultipleCollab(Vec<CollabMessage>),
}

impl RealtimeMessage {
  pub fn collab_messages(self) -> Vec<CollabMessage> {
    match self {
      RealtimeMessage::Collab(msg) => vec![msg],
      RealtimeMessage::MultipleCollab(msgs) => msgs,
      RealtimeMessage::User(_) => vec![],
      RealtimeMessage::System(_) => vec![],
    }
  }

  pub fn size(&self) -> usize {
    match self {
      RealtimeMessage::Collab(msg) => msg.len(),
      RealtimeMessage::MultipleCollab(msgs) => msgs.iter().map(|msg| msg.len()).sum(),
      RealtimeMessage::User(_) => 1,
      RealtimeMessage::System(_) => 1,
    }
  }

  pub fn is_collab_message(&self) -> bool {
    matches!(
      self,
      RealtimeMessage::Collab(_) | RealtimeMessage::MultipleCollab(_)
    )
  }
}

impl Display for RealtimeMessage {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      RealtimeMessage::Collab(msg) => f.write_fmt(format_args!("Collab:{}", msg)),
      RealtimeMessage::User(_) => f.write_fmt(format_args!("User")),
      RealtimeMessage::System(_) => f.write_fmt(format_args!("System")),
      RealtimeMessage::MultipleCollab(_) => f.write_fmt(format_args!("MultipleCollab")),
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

#[cfg(test)]
mod tests {
  use crate::collab_msg::{CollabMessage, InitSync};
  use crate::message::{RealtimeMessage, RealtimeMessageV1};
  use collab::core::origin::CollabOrigin;
  use collab_entity::CollabType;

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
    let version_2: RealtimeMessage = bincode::deserialize(&version_1_bytes).unwrap();

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

    let version_2_bytes = bincode::serialize(&version_2).unwrap();
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
