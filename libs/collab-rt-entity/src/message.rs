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
  ClientCollabV1(Vec<ClientCollabMessage>),
  ClientCollabV2(MessageByObjectId),
  ServerCollabV1(Vec<ServerCollabMessage>),
}

impl RealtimeMessage {
  pub fn size(&self) -> usize {
    match self {
      RealtimeMessage::Collab(msg) => msg.len(),
      RealtimeMessage::User(_) => 1,
      RealtimeMessage::System(_) => 1,
      RealtimeMessage::ClientCollabV1(msgs) => msgs.iter().map(|msg| msg.size()).sum(),
      RealtimeMessage::ClientCollabV2(msgs) => msgs
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
  pub fn transform(self) -> Result<MessageByObjectId, Error> {
    match self {
      RealtimeMessage::Collab(collab_message) => {
        let object_id = collab_message.object_id().to_string();
        let collab_message = ClientCollabMessage::try_from(collab_message)?;
        Ok([(object_id, vec![collab_message])].into())
      },
      RealtimeMessage::ClientCollabV1(collab_messages) => {
        let message_map: MessageByObjectId = collab_messages
          .into_iter()
          .map(|message| (message.object_id().to_string(), vec![message]))
          .collect();
        Ok(message_map)
      },
      RealtimeMessage::ClientCollabV2(collab_messages) => Ok(collab_messages),
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
      RealtimeMessage::ClientCollabV2(_) => f.write_fmt(format_args!("ClientCollabV2")),
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
  use crate::collab_msg::{ClientCollabMessage, CollabMessage, InitSync};
  use crate::message::{RealtimeMessage, SystemMessage};
  use crate::user::UserMessage;
  use bytes::Bytes;
  use collab::core::origin::CollabOrigin;
  use collab_entity::CollabType;
  use serde::{Deserialize, Serialize};
  use std::fs::File;
  use std::io::{Read, Write};

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
  fn decode_0149_realtime_message_test() {
    let collab_init = read_message_from_file("migration/0149/client_init").unwrap();
    assert!(matches!(collab_init, RealtimeMessage::Collab(_)));
    if let RealtimeMessage::Collab(CollabMessage::ClientInitSync(init)) = collab_init {
      assert_eq!(init.object_id, "object id 1");
      assert_eq!(init.collab_type, CollabType::Document);
      assert_eq!(init.workspace_id, "workspace id 1");
      assert_eq!(init.msg_id, 1);
      assert_eq!(init.payload, vec![1, 2, 3, 4]);
    } else {
      panic!("Failed to decode RealtimeMessage from file");
    }

    let collab_update = read_message_from_file("migration/0149/collab_update").unwrap();
    assert!(matches!(collab_update, RealtimeMessage::Collab(_)));
    if let RealtimeMessage::Collab(CollabMessage::ClientUpdateSync(update)) = collab_update {
      assert_eq!(update.object_id, "object id 1");
      assert_eq!(update.msg_id, 10);
      assert_eq!(update.payload, Bytes::from(vec![5, 6, 7, 8]));
    } else {
      panic!("Failed to decode RealtimeMessage from file");
    }

    let client_collab_v1 = read_message_from_file("migration/0149/client_collab_v1").unwrap();
    assert!(matches!(
      client_collab_v1,
      RealtimeMessage::ClientCollabV1(_)
    ));
    if let RealtimeMessage::ClientCollabV1(messages) = client_collab_v1 {
      assert_eq!(messages.len(), 1);
      if let ClientCollabMessage::ClientUpdateSync { data } = &messages[0] {
        assert_eq!(data.object_id, "object id 1");
        assert_eq!(data.msg_id, 10);
        assert_eq!(data.payload, Bytes::from(vec![5, 6, 7, 8]));
      } else {
        panic!("Failed to decode RealtimeMessage from file");
      }
    } else {
      panic!("Failed to decode RealtimeMessage from file");
    }
  }

  #[test]
  fn decode_0147_realtime_message_test() {
    let collab_init = read_message_from_file("migration/0147/client_init").unwrap();
    assert!(matches!(collab_init, RealtimeMessage::Collab(_)));
    if let RealtimeMessage::Collab(CollabMessage::ClientInitSync(init)) = collab_init {
      assert_eq!(init.object_id, "object id 1");
      assert_eq!(init.collab_type, CollabType::Document);
      assert_eq!(init.workspace_id, "workspace id 1");
      assert_eq!(init.msg_id, 1);
      assert_eq!(init.payload, vec![1, 2, 3, 4]);
    } else {
      panic!("Failed to decode RealtimeMessage from file");
    }

    let collab_update = read_message_from_file("migration/0147/collab_update").unwrap();
    assert!(matches!(collab_update, RealtimeMessage::Collab(_)));
    if let RealtimeMessage::Collab(CollabMessage::ClientUpdateSync(update)) = collab_update {
      assert_eq!(update.object_id, "object id 1");
      assert_eq!(update.msg_id, 10);
      assert_eq!(update.payload, Bytes::from(vec![5, 6, 7, 8]));
    } else {
      panic!("Failed to decode RealtimeMessage from file");
    }
  }

  #[test]
  fn decode_version_2_collab_message_with_version_1_test_1() {
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

  #[allow(dead_code)]
  fn write_message_to_file(
    message: &RealtimeMessage,
    file_path: &str,
  ) -> Result<(), Box<dyn std::error::Error>> {
    let data = message.encode().unwrap();
    let mut file = File::create(file_path)?;
    file.write_all(&data)?;
    Ok(())
  }

  #[allow(dead_code)]
  fn read_message_from_file(file_path: &str) -> Result<RealtimeMessage, anyhow::Error> {
    let mut file = File::open(file_path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let message = RealtimeMessage::decode(&buffer)?;
    Ok(message)
  }
}
