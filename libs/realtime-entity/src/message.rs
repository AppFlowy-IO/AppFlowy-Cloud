use crate::collab_msg::{ClientCollabMessage, CollabMessage, ServerCollabMessage};
use anyhow::{anyhow, Error};
use bincode::{DefaultOptions, Options};
use std::collections::HashMap;

use crate::user::UserMessage;
#[cfg(feature = "rt_compress")]
use brotli::{CompressorReader, Decompressor};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
#[cfg(feature = "rt_compress")]
use std::io::Read;

/// Maximum allowable size for a realtime message.
///
/// This sets the largest size a message can be for server processing in real-time communications.
/// If a message goes over this size, it won't be processed and will trigger a parser error.
/// This limit helps prevent server issues like overloads and denial-of-service attacks by rejecting
/// overly large messages.
pub const MAXIMUM_REALTIME_MESSAGE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB

/// 1 for using brotli compression
#[cfg(feature = "rt_compress")]
const COMPRESSED_PREFIX: &[u8] = b"COMPRESSED:1";

pub type MessageByObjectId = HashMap<String, Vec<ClientCollabMessage>>;

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
  ClientCollabV1(MessageByObjectId),
  ServerCollabV1(Vec<ServerCollabMessage>),
}

impl RealtimeMessage {
  pub fn size(&self) -> usize {
    match self {
      RealtimeMessage::Collab(msg) => msg.len(),
      RealtimeMessage::User(_) => 1,
      RealtimeMessage::System(_) => 1,
      RealtimeMessage::ClientCollabV1(msgs) => msgs
        .iter()
        .map(|(_, value)| value.iter().map(|v| v.size()).sum::<usize>())
        .sum(),
      RealtimeMessage::ServerCollabV1(msgs) => msgs.iter().map(|msg| msg.size()).sum(),
    }
  }

  /// Convert RealtimeMessage to ClientCollabMessage
  /// If the message is not a collab message, it will return an empty vec
  /// If the message is a collab message, it will return a vec with one element
  /// If the message is a ClientCollabV1, it will return list of collab messages
  pub fn try_into_message_by_object_id(self) -> Result<MessageByObjectId, Error> {
    match self {
      RealtimeMessage::Collab(collab_message) => {
        let object_id = collab_message.object_id().to_string();
        let collab_message = ClientCollabMessage::try_from(collab_message)?;
        Ok([(object_id, vec![collab_message])].into())
      },
      RealtimeMessage::ClientCollabV1(collab_messages) => Ok(collab_messages),
      _ => Err(anyhow!(
        "Failed to convert RealtimeMessage:{} to ClientCollabMessage",
        self
      )),
    }
  }

  #[cfg(feature = "rt_compress")]
  pub fn encode(&self) -> Result<Vec<u8>, Error> {
    let data = DefaultOptions::new()
      .with_fixint_encoding()
      .allow_trailing_bytes()
      .with_limit(MAXIMUM_REALTIME_MESSAGE_SIZE)
      .serialize(self)?;

    let mut compressor = CompressorReader::new(&*data, 4096, 4, 22);
    let mut compressed_data = Vec::new();
    compressor.read_to_end(&mut compressed_data)?;
    let mut data = Vec::new();
    data.extend_from_slice(COMPRESSED_PREFIX);
    data.extend(compressed_data);
    Ok(data)
  }

  #[cfg(not(feature = "rt_compress"))]
  pub fn encode(&self) -> Result<Vec<u8>, Error> {
    let data = DefaultOptions::new()
      .with_fixint_encoding()
      .allow_trailing_bytes()
      .with_limit(MAXIMUM_REALTIME_MESSAGE_SIZE)
      .serialize(self)?;
    Ok(data)
  }

  #[cfg(feature = "rt_compress")]
  pub fn decode(data: &[u8]) -> Result<Self, Error> {
    if data.starts_with(COMPRESSED_PREFIX) {
      let data_without_prefix = &data[COMPRESSED_PREFIX.len()..];
      let mut decompressor = Decompressor::new(data_without_prefix, 4096);
      let mut decompressed_data = Vec::new();
      decompressor.read_to_end(&mut decompressed_data)?;
      decompressed_data
    } else {
      data.to_vec()
    }

    let message = DefaultOptions::new()
      .with_fixint_encoding()
      .allow_trailing_bytes()
      .with_limit(MAXIMUM_REALTIME_MESSAGE_SIZE)
      .deserialize(&data)?;
    Ok(message)
  }

  #[cfg(not(feature = "rt_compress"))]
  pub fn decode(data: &[u8]) -> Result<Self, Error> {
    let message = DefaultOptions::new()
      .with_fixint_encoding()
      .allow_trailing_bytes()
      .with_limit(MAXIMUM_REALTIME_MESSAGE_SIZE)
      .deserialize(data)?;
    Ok(message)
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

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum SystemMessage {
  RateLimit(u32),
  KickOff,
  DuplicateConnection,
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
    let version_2 = RealtimeMessage::decode(&version_1_bytes).unwrap();

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

    let version_2_bytes = version_2.encode().unwrap();
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
