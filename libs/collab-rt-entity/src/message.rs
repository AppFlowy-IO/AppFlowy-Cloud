use anyhow::{Error, anyhow};
use bincode::{DefaultOptions, Options};
use std::collections::HashMap;

use crate::client_message::ClientCollabMessage;
use crate::server_message::ServerCollabMessage;
use crate::user::UserMessage;
use crate::{AwarenessSync, BroadcastSync, CollabAck, InitSync, ServerInit, UpdateSync};
use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::ops::{Deref, DerefMut};

/// Maximum allowable size for a realtime message.
///
/// This sets the largest size a message can be for server processing in real-time communications.
/// If a message goes over this size, it won't be processed and will trigger a parser error.
/// This limit helps prevent server issues like overloads and denial-of-service attacks by rejecting
/// overly large messages.
pub const MAXIMUM_REALTIME_MESSAGE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MessageByObjectId(pub HashMap<String, Vec<ClientCollabMessage>>);
impl MessageByObjectId {
  pub fn new_with_message(object_id: String, messages: Vec<ClientCollabMessage>) -> Self {
    let mut map = HashMap::with_capacity(1);
    map.insert(object_id, messages);
    Self(map)
  }

  pub fn into_inner(self) -> HashMap<String, Vec<ClientCollabMessage>> {
    self.0
  }
}
impl Deref for MessageByObjectId {
  type Target = HashMap<String, Vec<ClientCollabMessage>>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for MessageByObjectId {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
  feature = "actix_message",
  derive(actix::Message),
  rtype(result = "()")
)]
pub enum RealtimeMessage {
  Collab(CollabMessage), // Deprecated
  User(UserMessage),
  System(SystemMessage),
  ClientCollabV1(Vec<ClientCollabMessage>), // Deprecated
  ClientCollabV2(MessageByObjectId),
  ServerCollabV1(Vec<ServerCollabMessage>),
}

impl RealtimeMessage {
  /// Convert RealtimeMessage to ClientCollabMessage
  /// If the message is not a collab message, it will return an empty vec
  /// If the message is a collab message, it will return a vec with one element
  /// If the message is a ClientCollabV1, it will return list of collab messages
  pub fn split_messages_by_object_id(self) -> Result<MessageByObjectId, Error> {
    match self {
      RealtimeMessage::Collab(collab_message) => {
        let object_id = collab_message.object_id().to_string();
        let message = MessageByObjectId::new_with_message(
          object_id,
          vec![ClientCollabMessage::try_from(collab_message)?],
        );
        Ok(message)
      },
      RealtimeMessage::ClientCollabV1(_) => Err(anyhow!("ClientCollabV1 is not supported")),
      RealtimeMessage::ClientCollabV2(collab_messages) => Ok(collab_messages),
      _ => Err(anyhow!(
        "Failed to convert RealtimeMessage:{} to ClientCollabMessage",
        self
      )),
    }
  }

  fn object_id(&self) -> Option<String> {
    match self {
      RealtimeMessage::Collab(msg) => Some(msg.object_id().to_string()),
      RealtimeMessage::ClientCollabV1(msgs) => msgs.first().map(|msg| msg.object_id().to_string()),
      RealtimeMessage::ClientCollabV2(msgs) => {
        if let Some((object_id, _)) = msgs.iter().next() {
          Some(object_id.to_string())
        } else {
          None
        }
      },
      _ => None,
    }
  }

  pub fn encode(&self) -> Result<Vec<u8>, Error> {
    let data = DefaultOptions::new()
      .with_fixint_encoding()
      .allow_trailing_bytes()
      .with_limit(MAXIMUM_REALTIME_MESSAGE_SIZE)
      .serialize(self)
      .map_err(|e| {
        anyhow!(
          "Failed to encode realtime message: {}, object_id:{:?}",
          e,
          self.object_id()
        )
      })?;
    Ok(data)
  }

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

impl CollabMessage {
  pub fn msg_id(&self) -> Option<MsgId> {
    match self {
      CollabMessage::ClientInitSync(value) => Some(value.msg_id),
      CollabMessage::ClientUpdateSync(value) => Some(value.msg_id),
      CollabMessage::ClientAck(value) => Some(value.msg_id),
      CollabMessage::ServerInitSync(value) => Some(value.msg_id),
      CollabMessage::ServerBroadcast(_) => None,
      CollabMessage::AwarenessSync(_) => None,
    }
  }

  pub fn len(&self) -> usize {
    self.payload().len()
  }
  pub fn payload(&self) -> &Bytes {
    match self {
      CollabMessage::ClientInitSync(value) => &value.payload,
      CollabMessage::ClientUpdateSync(value) => &value.payload,
      CollabMessage::ClientAck(value) => &value.payload,
      CollabMessage::ServerInitSync(value) => &value.payload,
      CollabMessage::ServerBroadcast(value) => &value.payload,
      CollabMessage::AwarenessSync(value) => &value.payload,
    }
  }
  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }
  pub fn origin(&self) -> &CollabOrigin {
    match self {
      CollabMessage::ClientInitSync(value) => &value.origin,
      CollabMessage::ClientUpdateSync(value) => &value.origin,
      CollabMessage::ClientAck(value) => &value.origin,
      CollabMessage::ServerInitSync(value) => &value.origin,
      CollabMessage::ServerBroadcast(value) => &value.origin,
      CollabMessage::AwarenessSync(value) => &value.origin,
    }
  }

  pub fn uid(&self) -> Option<i64> {
    self.origin().client_user_id()
  }

  pub fn object_id(&self) -> &str {
    match self {
      CollabMessage::ClientInitSync(value) => &value.object_id,
      CollabMessage::ClientUpdateSync(value) => &value.object_id,
      CollabMessage::ClientAck(value) => &value.object_id,
      CollabMessage::ServerInitSync(value) => &value.object_id,
      CollabMessage::ServerBroadcast(value) => &value.object_id,
      CollabMessage::AwarenessSync(value) => &value.object_id,
    }
  }
}

impl Display for CollabMessage {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      CollabMessage::ClientInitSync(value) => Display::fmt(&value, f),
      CollabMessage::ClientUpdateSync(value) => Display::fmt(&value, f),
      CollabMessage::ClientAck(value) => Display::fmt(&value, f),
      CollabMessage::ServerInitSync(value) => Display::fmt(&value, f),
      CollabMessage::ServerBroadcast(value) => Display::fmt(&value, f),
      CollabMessage::AwarenessSync(value) => Display::fmt(&value, f),
    }
  }
}

impl From<CollabAck> for CollabMessage {
  fn from(value: CollabAck) -> Self {
    CollabMessage::ClientAck(value)
  }
}

impl From<BroadcastSync> for CollabMessage {
  fn from(value: BroadcastSync) -> Self {
    CollabMessage::ServerBroadcast(value)
  }
}

impl From<InitSync> for CollabMessage {
  fn from(value: InitSync) -> Self {
    CollabMessage::ClientInitSync(value)
  }
}

impl From<UpdateSync> for CollabMessage {
  fn from(value: UpdateSync) -> Self {
    CollabMessage::ClientUpdateSync(value)
  }
}

impl From<AwarenessSync> for CollabMessage {
  fn from(value: AwarenessSync) -> Self {
    CollabMessage::AwarenessSync(value)
  }
}

impl From<ServerInit> for CollabMessage {
  fn from(value: ServerInit) -> Self {
    CollabMessage::ServerInitSync(value)
  }
}

impl TryFrom<RealtimeMessage> for CollabMessage {
  type Error = anyhow::Error;

  fn try_from(value: RealtimeMessage) -> Result<Self, Self::Error> {
    match value {
      RealtimeMessage::Collab(msg) => Ok(msg),
      _ => Err(anyhow!("Invalid message type.")),
    }
  }
}

impl From<CollabMessage> for RealtimeMessage {
  fn from(msg: CollabMessage) -> Self {
    Self::Collab(msg)
  }
}
