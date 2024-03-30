use crate::message::RealtimeMessage;
use crate::{CollabMessage, MsgId};
use anyhow::{anyhow, Error};
use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use serde::de::Visitor;
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_repr::Serialize_repr;
use std::fmt;
use std::fmt::{Display, Formatter};

#[derive(Clone, Debug, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum ServerCollabMessage {
  ClientAck(CollabAck),
  ServerInitSync(ServerInit),
  AwarenessSync(AwarenessSync),
  ServerBroadcast(BroadcastSync),
}

impl ServerCollabMessage {
  pub fn object_id(&self) -> &str {
    match self {
      ServerCollabMessage::ClientAck(value) => &value.object_id,
      ServerCollabMessage::ServerInitSync(value) => &value.object_id,
      ServerCollabMessage::AwarenessSync(value) => &value.object_id,
      ServerCollabMessage::ServerBroadcast(value) => &value.object_id,
    }
  }

  pub fn msg_id(&self) -> Option<MsgId> {
    match self {
      ServerCollabMessage::ClientAck(value) => Some(value.msg_id),
      ServerCollabMessage::ServerInitSync(value) => Some(value.msg_id),
      ServerCollabMessage::AwarenessSync(_) => None,
      ServerCollabMessage::ServerBroadcast(_) => None,
    }
  }

  pub fn payload(&self) -> &Bytes {
    match self {
      ServerCollabMessage::ClientAck(value) => &value.payload,
      ServerCollabMessage::ServerInitSync(value) => &value.payload,
      ServerCollabMessage::AwarenessSync(value) => &value.payload,
      ServerCollabMessage::ServerBroadcast(value) => &value.payload,
    }
  }

  pub fn size(&self) -> usize {
    match self {
      ServerCollabMessage::ClientAck(msg) => msg.payload.len(),
      ServerCollabMessage::ServerInitSync(msg) => msg.payload.len(),
      ServerCollabMessage::AwarenessSync(msg) => msg.payload.len(),
      ServerCollabMessage::ServerBroadcast(msg) => msg.payload.len(),
    }
  }

  pub fn origin(&self) -> &CollabOrigin {
    match self {
      ServerCollabMessage::ClientAck(value) => &value.origin,
      ServerCollabMessage::ServerInitSync(value) => &value.origin,
      ServerCollabMessage::AwarenessSync(value) => &value.origin,
      ServerCollabMessage::ServerBroadcast(value) => &value.origin,
    }
  }
}

impl Display for ServerCollabMessage {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      ServerCollabMessage::ClientAck(value) => Display::fmt(&value, f),
      ServerCollabMessage::ServerInitSync(value) => Display::fmt(&value, f),
      ServerCollabMessage::AwarenessSync(value) => Display::fmt(&value, f),
      ServerCollabMessage::ServerBroadcast(value) => Display::fmt(&value, f),
    }
  }
}

impl TryFrom<CollabMessage> for ServerCollabMessage {
  type Error = Error;

  fn try_from(value: CollabMessage) -> Result<Self, Self::Error> {
    match value {
      CollabMessage::ClientAck(msg) => Ok(ServerCollabMessage::ClientAck(msg)),
      CollabMessage::ServerInitSync(msg) => Ok(ServerCollabMessage::ServerInitSync(msg)),
      CollabMessage::AwarenessSync(msg) => Ok(ServerCollabMessage::AwarenessSync(msg)),
      CollabMessage::ServerBroadcast(msg) => Ok(ServerCollabMessage::ServerBroadcast(msg)),
      _ => Err(anyhow!("Invalid collab message type.")),
    }
  }
}

impl From<ServerCollabMessage> for RealtimeMessage {
  fn from(msg: ServerCollabMessage) -> Self {
    Self::ServerCollabV1(vec![msg])
  }
}

impl From<ServerInit> for ServerCollabMessage {
  fn from(value: ServerInit) -> Self {
    ServerCollabMessage::ServerInitSync(value)
  }
}

///  ⚠️ ⚠️ ⚠️Compatibility Warning:
///
/// The structure of this struct is integral to maintaining compatibility with existing messages.
/// Therefore, adding or removing any properties (fields) from this struct could disrupt the
/// compatibility. Such changes may lead to issues in processing existing messages that expect
/// the struct to have a specific format. It's crucial to carefully consider the implications
/// of modifying this struct's fields
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize, Hash)]
pub struct ServerInit {
  pub origin: CollabOrigin,
  pub object_id: String,
  pub msg_id: MsgId,
  /// "The payload is encoded using the `EncoderV1` with the `Message` struct.
  /// To decode the message, use the `MessageReader`."
  /// ```text
  ///   let mut decoder = DecoderV1::new(Cursor::new(payload));
  ///   let reader = MessageReader::new(&mut decoder);
  ///   for message in reader {
  ///    ...
  ///   }
  /// ```
  pub payload: Bytes,
}

impl ServerInit {
  pub fn new(origin: CollabOrigin, object_id: String, payload: Vec<u8>, msg_id: MsgId) -> Self {
    Self {
      origin,
      object_id,
      payload: Bytes::from(payload),
      msg_id,
    }
  }
}

impl Display for ServerInit {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "server init: [uid:{}|oid:{}|msg_id:{:?}|len:{}]",
      self.origin.client_user_id().unwrap_or(0),
      self.object_id,
      self.msg_id,
      self.payload.len(),
    ))
  }
}

///  ⚠️ ⚠️ ⚠️Compatibility Warning:
///
/// The structure of this struct is integral to maintaining compatibility with existing messages.
/// Therefore, adding or removing any properties (fields) from this struct could disrupt the
/// compatibility. Such changes may lead to issues in processing existing messages that expect
/// the struct to have a specific format. It's crucial to carefully consider the implications
/// of modifying this struct's fields
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize, Hash)]
pub struct CollabAck {
  pub origin: CollabOrigin,
  pub object_id: String,
  #[deprecated(note = "since 0.2.18")]
  pub meta: AckMeta,
  pub payload: Bytes,
  #[serde(deserialize_with = "deserialize_ack_code")]
  pub code: AckCode,
  pub msg_id: MsgId,
  pub seq_num: u32,
}

impl CollabAck {
  #[allow(deprecated)]
  pub fn new(origin: CollabOrigin, object_id: String, msg_id: MsgId, seq_num: u32) -> Self {
    Self {
      origin,
      object_id,
      meta: AckMeta::new(&msg_id),
      payload: Bytes::from(vec![]),
      code: AckCode::Success,
      msg_id,
      seq_num,
    }
  }

  pub fn with_payload<T: Into<Bytes>>(mut self, payload: T) -> Self {
    self.payload = payload.into();
    self
  }

  pub fn with_code(mut self, code: AckCode) -> Self {
    self.code = code;
    self
  }
}

fn deserialize_ack_code<'de, D>(deserializer: D) -> Result<AckCode, D::Error>
where
  D: Deserializer<'de>,
{
  struct AckCodeVisitor;

  impl<'de> Visitor<'de> for AckCodeVisitor {
    type Value = AckCode;

    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
      formatter.write_str("an integer between 0 and 4")
    }

    fn visit_u8<E>(self, value: u8) -> Result<Self::Value, E>
    where
      E: de::Error,
    {
      match value {
        0 => Ok(AckCode::Success),
        1 => Ok(AckCode::CannotApplyUpdate),
        2 => Ok(AckCode::Retry),
        3 => Ok(AckCode::Internal),
        4 => Ok(AckCode::EncodeState),
        _ => Ok(AckCode::Internal),
      }
    }
  }

  deserializer.deserialize_u8(AckCodeVisitor)
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize_repr, Hash)]
#[repr(u8)]
pub enum AckCode {
  Success = 0,
  CannotApplyUpdate = 1,
  Retry = 2,
  Internal = 3,
  EncodeState = 4,
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize, Hash)]
pub struct AckMeta {
  pub data: String,
  pub msg_id: MsgId,
}

impl AckMeta {
  fn new(msg_id: &MsgId) -> Self {
    Self {
      data: "".to_string(),
      msg_id: *msg_id,
    }
  }
}

impl Display for CollabAck {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "ack: [uid:{}|oid:{}|msg_id:{:?}|len:{}]",
      self.origin.client_user_id().unwrap_or(0),
      self.object_id,
      self.msg_id,
      self.payload.len(),
    ))
  }
}

///  ⚠️ ⚠️ ⚠️Compatibility Warning:
///
/// The structure of this struct is integral to maintaining compatibility with existing messages.
/// Therefore, adding or removing any properties (fields) from this struct could disrupt the
/// compatibility. Such changes may lead to issues in processing existing messages that expect
/// the struct to have a specific format. It's crucial to carefully consider the implications
/// of modifying this struct's fields
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize, Hash)]
pub struct BroadcastSync {
  pub(crate) origin: CollabOrigin,
  pub(crate) object_id: String,
  /// "The payload is encoded using the `EncoderV1` with the `Message` struct.
  /// It can be parsed into: Message::Sync::(SyncMessage::Update(update))
  pub(crate) payload: Bytes,
  pub seq_num: u32,
}

impl BroadcastSync {
  pub fn new(origin: CollabOrigin, object_id: String, payload: Vec<u8>, seq_num: u32) -> Self {
    Self {
      origin,
      object_id,
      payload: Bytes::from(payload),
      seq_num,
    }
  }
}

impl Display for BroadcastSync {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "broadcast: [oid:{}|len:{}]",
      self.object_id,
      self.payload.len(),
    ))
  }
}

///  ⚠️ ⚠️ ⚠️Compatibility Warning:
///
/// The structure of this struct is integral to maintaining compatibility with existing messages.
/// Therefore, adding or removing any properties (fields) from this struct could disrupt the
/// compatibility. Such changes may lead to issues in processing existing messages that expect
/// the struct to have a specific format. It's crucial to carefully consider the implications
/// of modifying this struct's fields
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize, Hash)]
pub struct AwarenessSync {
  pub(crate) object_id: String,
  pub(crate) payload: Bytes,
  pub(crate) origin: CollabOrigin,
}

impl AwarenessSync {
  pub fn new(object_id: String, payload: Vec<u8>) -> Self {
    Self {
      object_id,
      payload: Bytes::from(payload),
      origin: CollabOrigin::Server,
    }
  }
}

impl Display for AwarenessSync {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "awareness: [|oid:{}|len:{}]",
      self.object_id,
      self.payload.len(),
    ))
  }
}
