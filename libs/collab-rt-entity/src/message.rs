use anyhow::{anyhow, Error};
use bincode::{DefaultOptions, Options};
use std::collections::HashMap;

use crate::client_message::ClientCollabMessage;
use crate::server_message::ServerCollabMessage;
use crate::user::UserMessage;
use crate::{AwarenessSync, BroadcastSync, CollabAck, InitSync, ServerInit, UpdateSync};
#[cfg(feature = "rt_compress")]
use brotli::{CompressorReader, Decompressor};
use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
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

pub type MsgId = u64;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CollabMessage {
  ClientInitSync(InitSync),
  ClientUpdateSync(UpdateSync),
  ClientAck(CollabAck),
  ServerInitSync(ServerInit),
  AwarenessSync(AwarenessSync),
  ServerBroadcast(BroadcastSync),
}

impl crate::CollabMessage {
  pub fn msg_id(&self) -> Option<crate::MsgId> {
    match self {
      crate::CollabMessage::ClientInitSync(value) => Some(value.msg_id),
      crate::CollabMessage::ClientUpdateSync(value) => Some(value.msg_id),
      crate::CollabMessage::ClientAck(value) => Some(value.msg_id),
      crate::CollabMessage::ServerInitSync(value) => Some(value.msg_id),
      crate::CollabMessage::ServerBroadcast(_) => None,
      crate::CollabMessage::AwarenessSync(_) => None,
    }
  }

  pub fn len(&self) -> usize {
    self.payload().len()
  }
  pub fn payload(&self) -> &Bytes {
    match self {
      crate::CollabMessage::ClientInitSync(value) => &value.payload,
      crate::CollabMessage::ClientUpdateSync(value) => &value.payload,
      crate::CollabMessage::ClientAck(value) => &value.payload,
      crate::CollabMessage::ServerInitSync(value) => &value.payload,
      crate::CollabMessage::ServerBroadcast(value) => &value.payload,
      crate::CollabMessage::AwarenessSync(value) => &value.payload,
    }
  }
  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }
  pub fn origin(&self) -> &CollabOrigin {
    match self {
      crate::CollabMessage::ClientInitSync(value) => &value.origin,
      crate::CollabMessage::ClientUpdateSync(value) => &value.origin,
      crate::CollabMessage::ClientAck(value) => &value.origin,
      crate::CollabMessage::ServerInitSync(value) => &value.origin,
      crate::CollabMessage::ServerBroadcast(value) => &value.origin,
      crate::CollabMessage::AwarenessSync(value) => &value.origin,
    }
  }

  pub fn uid(&self) -> Option<i64> {
    self.origin().client_user_id()
  }

  pub fn object_id(&self) -> &str {
    match self {
      crate::CollabMessage::ClientInitSync(value) => &value.object_id,
      crate::CollabMessage::ClientUpdateSync(value) => &value.object_id,
      crate::CollabMessage::ClientAck(value) => &value.object_id,
      crate::CollabMessage::ServerInitSync(value) => &value.object_id,
      crate::CollabMessage::ServerBroadcast(value) => &value.object_id,
      crate::CollabMessage::AwarenessSync(value) => &value.object_id,
    }
  }
}

impl Display for crate::CollabMessage {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      crate::CollabMessage::ClientInitSync(value) => Display::fmt(&value, f),
      crate::CollabMessage::ClientUpdateSync(value) => Display::fmt(&value, f),
      crate::CollabMessage::ClientAck(value) => Display::fmt(&value, f),
      crate::CollabMessage::ServerInitSync(value) => Display::fmt(&value, f),
      crate::CollabMessage::ServerBroadcast(value) => Display::fmt(&value, f),
      crate::CollabMessage::AwarenessSync(value) => Display::fmt(&value, f),
    }
  }
}

impl From<ClientCollabMessage> for crate::CollabMessage {
  fn from(value: ClientCollabMessage) -> Self {
    match value {
      ClientCollabMessage::ClientInitSync { data, .. } => {
        crate::CollabMessage::ClientInitSync(data)
      },
      ClientCollabMessage::ClientUpdateSync { data, .. } => {
        crate::CollabMessage::ClientUpdateSync(data)
      },
      ClientCollabMessage::ServerInitSync(data) => crate::CollabMessage::ServerInitSync(data),
      ClientCollabMessage::ClientAwarenessSync(data) => {
        crate::CollabMessage::ClientUpdateSync(data)
      },
    }
  }
}

impl From<CollabAck> for crate::CollabMessage {
  fn from(value: CollabAck) -> Self {
    crate::CollabMessage::ClientAck(value)
  }
}

impl From<BroadcastSync> for crate::CollabMessage {
  fn from(value: BroadcastSync) -> Self {
    crate::CollabMessage::ServerBroadcast(value)
  }
}

impl From<InitSync> for crate::CollabMessage {
  fn from(value: InitSync) -> Self {
    crate::CollabMessage::ClientInitSync(value)
  }
}

impl From<UpdateSync> for crate::CollabMessage {
  fn from(value: UpdateSync) -> Self {
    crate::CollabMessage::ClientUpdateSync(value)
  }
}

impl From<AwarenessSync> for crate::CollabMessage {
  fn from(value: AwarenessSync) -> Self {
    crate::CollabMessage::AwarenessSync(value)
  }
}

impl From<ServerInit> for crate::CollabMessage {
  fn from(value: ServerInit) -> Self {
    crate::CollabMessage::ServerInitSync(value)
  }
}

impl TryFrom<RealtimeMessage> for crate::CollabMessage {
  type Error = anyhow::Error;

  fn try_from(value: RealtimeMessage) -> Result<Self, Self::Error> {
    match value {
      RealtimeMessage::Collab(msg) => Ok(msg),
      _ => Err(anyhow!("Invalid message type.")),
    }
  }
}

impl From<crate::CollabMessage> for RealtimeMessage {
  fn from(msg: crate::CollabMessage) -> Self {
    Self::Collab(msg)
  }
}
