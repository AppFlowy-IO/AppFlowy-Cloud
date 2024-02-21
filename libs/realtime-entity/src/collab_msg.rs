use anyhow::{anyhow, Error};
use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter};

use crate::message::RealtimeMessage;
use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use collab::preclude::merge_updates_v1;
use collab::preclude::updates::decoder::DecoderV1;
use collab::preclude::updates::encoder::{Encode, Encoder, EncoderV1};
use collab_entity::CollabType;
use realtime_protocol::{Message, MessageReader, SyncMessage};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

pub trait CollabSinkMessage: Clone + Send + Sync + 'static + Ord + Display {
  fn collab_object_id(&self) -> &str;
  /// Returns the length of the message in bytes.
  fn payload_len(&self) -> usize;

  /// Returns true if the message can be merged with other messages.
  fn can_merge(&self) -> bool;

  fn merge(&mut self, other: &Self, maximum_payload_size: &usize) -> Result<bool, Error>;

  fn is_init_msg(&self) -> bool;
}

pub type MsgId = u64;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CollabMessage {
  ClientInitSync(InitSync),
  ClientUpdateSync(UpdateSync),
  ClientAck(CollabAck),
  ServerInitSync(ServerInit),
  AwarenessSync(CollabAwareness),
  ServerBroadcast(CollabBroadcastData),
}

impl CollabSinkMessage for CollabMessage {
  fn collab_object_id(&self) -> &str {
    self.object_id()
  }

  fn payload_len(&self) -> usize {
    self.len()
  }

  fn can_merge(&self) -> bool {
    matches!(self, CollabMessage::ClientUpdateSync(_))
  }

  fn merge(&mut self, other: &Self, maximum_payload_size: &usize) -> Result<bool, Error> {
    match (self, other) {
      (CollabMessage::ClientUpdateSync(value), CollabMessage::ClientUpdateSync(other)) => {
        if &value.payload.len() > maximum_payload_size {
          Ok(false)
        } else {
          value.merge_payload(other)
        }
      },
      _ => Ok(false),
    }
  }

  fn is_init_msg(&self) -> bool {
    matches!(self, CollabMessage::ClientInitSync(_))
  }
}

impl Eq for CollabMessage {}

impl PartialEq for CollabMessage {
  fn eq(&self, other: &Self) -> bool {
    self.msg_id() == other.msg_id()
  }
}

impl PartialOrd for CollabMessage {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for CollabMessage {
  fn cmp(&self, other: &Self) -> Ordering {
    match (&self, &other) {
      (CollabMessage::ClientInitSync { .. }, CollabMessage::ClientInitSync { .. }) => {
        Ordering::Equal
      },
      (CollabMessage::ClientInitSync { .. }, _) => Ordering::Greater,
      (_, CollabMessage::ClientInitSync { .. }) => Ordering::Less,
      (CollabMessage::ServerInitSync(_), CollabMessage::ServerInitSync(_)) => Ordering::Equal,
      (CollabMessage::ServerInitSync { .. }, _) => Ordering::Greater,
      (_, CollabMessage::ServerInitSync { .. }) => Ordering::Less,
      _ => self.msg_id().cmp(&other.msg_id()).reverse(),
    }
  }
}

impl CollabMessage {
  pub fn is_client_init(&self) -> bool {
    matches!(self, CollabMessage::ClientInitSync(_))
  }
  pub fn is_server_init(&self) -> bool {
    matches!(self, CollabMessage::ServerInitSync(_))
  }

  pub fn type_str(&self) -> String {
    match self {
      CollabMessage::ClientInitSync(_) => "ClientInitSync".to_string(),
      CollabMessage::ClientUpdateSync(_) => "UpdateSync".to_string(),
      CollabMessage::ClientAck(_) => "ClientAck".to_string(),
      CollabMessage::ServerInitSync(_) => "ServerInitSync".to_string(),
      CollabMessage::ServerBroadcast(_) => "Broadcast".to_string(),
      CollabMessage::AwarenessSync(_) => "Awareness".to_string(),
    }
  }

  pub fn msg_id(&self) -> Option<MsgId> {
    match self {
      CollabMessage::ClientInitSync(value) => Some(value.msg_id),
      CollabMessage::ClientUpdateSync(value) => Some(value.msg_id),
      CollabMessage::ClientAck(value) => Some(value.source.msg_id),
      CollabMessage::ServerInitSync(value) => Some(value.msg_id),
      CollabMessage::ServerBroadcast(_) => None,
      CollabMessage::AwarenessSync(_) => None,
    }
  }

  pub fn len(&self) -> usize {
    self.payload().map(|payload| payload.len()).unwrap_or(0)
  }
  pub fn payload(&self) -> Option<&Bytes> {
    match self {
      CollabMessage::ClientInitSync(value) => Some(&value.payload),
      CollabMessage::ClientUpdateSync(value) => Some(&value.payload),
      CollabMessage::ClientAck(value) => Some(&value.payload),
      CollabMessage::ServerInitSync(value) => Some(&value.payload),
      CollabMessage::ServerBroadcast(value) => Some(&value.payload),
      CollabMessage::AwarenessSync(value) => Some(&value.payload),
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

  pub fn device_id(&self) -> Option<String> {
    match self.origin() {
      CollabOrigin::Client(origin) => Some(origin.device_id.clone()),
      _ => None,
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

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct CollabAwareness {
  object_id: String,
  payload: Bytes,
  origin: CollabOrigin,
}

impl CollabAwareness {
  pub fn new(object_id: String, payload: Vec<u8>) -> Self {
    Self {
      object_id,
      payload: Bytes::from(payload),
      origin: CollabOrigin::Server,
    }
  }
}

impl From<CollabAwareness> for CollabMessage {
  fn from(value: CollabAwareness) -> Self {
    CollabMessage::AwarenessSync(value)
  }
}

impl Display for CollabAwareness {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "awareness: [oid:{}|len:{}]",
      self.object_id,
      self.payload.len(),
    ))
  }
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct InitSync {
  pub origin: CollabOrigin,
  pub object_id: String,
  pub collab_type: CollabType,
  pub workspace_id: String,
  pub msg_id: MsgId,
  pub payload: Bytes,
}

impl InitSync {
  pub fn new(
    origin: CollabOrigin,
    object_id: String,
    collab_type: CollabType,
    workspace_id: String,
    msg_id: MsgId,
    payload: Vec<u8>,
  ) -> Self {
    let payload = Bytes::from(payload);
    Self {
      origin,
      object_id,
      collab_type,
      workspace_id,
      msg_id,
      payload,
    }
  }
}

impl Display for InitSync {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "client init: [{}|oid:{}|msg_id:{}|len:{}]",
      self.origin,
      self.object_id,
      self.msg_id,
      self.payload.len(),
    ))
  }
}

impl From<InitSync> for CollabMessage {
  fn from(value: InitSync) -> Self {
    CollabMessage::ClientInitSync(value)
  }
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
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

impl From<ServerInit> for CollabMessage {
  fn from(value: ServerInit) -> Self {
    CollabMessage::ServerInitSync(value)
  }
}

impl Display for ServerInit {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "server init: [origin:{}|oid:{}|msg_id:{:?}|len:{}]",
      self.origin,
      self.object_id,
      self.msg_id,
      self.payload.len(),
    ))
  }
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct UpdateSync {
  pub origin: CollabOrigin,
  pub object_id: String,
  pub msg_id: MsgId,
  /// "The payload is encoded using the `EncoderV1` with the `Message` struct.
  /// Message::Sync(SyncMessage::Update(update)).encode_v1()
  ///
  /// we can using the `MessageReader` to decode the payload
  /// ```text
  ///   let mut decoder = DecoderV1::new(Cursor::new(payload));
  ///   let reader = MessageReader::new(&mut decoder);
  ///   for message in reader {
  ///    ...
  ///   }
  /// ```
  ///  
  pub payload: Bytes,
}

impl UpdateSync {
  pub fn new(origin: CollabOrigin, object_id: String, payload: Vec<u8>, msg_id: MsgId) -> Self {
    Self {
      origin,
      object_id,
      payload: Bytes::from(payload),
      msg_id,
    }
  }

  pub fn merge_payload(&mut self, other: &Self) -> Result<bool, Error> {
    // TODO(nathan): optimize the merge process
    if let (
      Some(Message::Sync(SyncMessage::Update(left))),
      Some(Message::Sync(SyncMessage::Update(right))),
    ) = (self.as_update(), other.as_update())
    {
      let update = merge_updates_v1(&[&left, &right])?;
      let msg = Message::Sync(SyncMessage::Update(update));
      let mut encoder = EncoderV1::new();
      msg.encode(&mut encoder);
      self.payload = Bytes::from(encoder.to_vec());
      Ok(true)
    } else {
      Ok(false)
    }
  }

  fn as_update(&self) -> Option<Message> {
    let mut decoder = DecoderV1::from(self.payload.as_ref());
    let mut reader = MessageReader::new(&mut decoder);
    reader.next()?.ok()
  }
}

impl From<UpdateSync> for CollabMessage {
  fn from(value: UpdateSync) -> Self {
    CollabMessage::ClientUpdateSync(value)
  }
}

impl Display for UpdateSync {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "client update sync: [origin:{}|oid:{}|msg_id:{:?}|len:{}]",
      self.origin,
      self.object_id,
      self.msg_id,
      self.payload.len(),
    ))
  }
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum AckCode {
  Success = 0,
  CannotApplyUpdate = 1,
  Retry = 2,
  Internal = 3,
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct CollabAck {
  pub origin: CollabOrigin,
  pub object_id: String,
  pub source: AckSource,
  pub payload: Bytes,
  pub code: AckCode,
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct AckSource {
  #[serde(rename = "sync_verbose")]
  pub verbose: String,
  pub msg_id: MsgId,
}

impl CollabAck {
  pub fn new(origin: CollabOrigin, object_id: String, msg_id: MsgId) -> Self {
    let source = AckSource {
      verbose: "".to_string(),
      msg_id,
    };
    Self {
      origin,
      object_id,
      source,
      payload: Bytes::from(vec![]),
      code: AckCode::Success,
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

impl From<CollabAck> for CollabMessage {
  fn from(value: CollabAck) -> Self {
    CollabMessage::ClientAck(value)
  }
}

impl Display for CollabAck {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "ack: [origin:{}|oid:{}|msg_id:{:?}|len:{}]",
      self.origin,
      self.object_id,
      self.source.msg_id,
      self.payload.len(),
    ))
  }
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct CollabBroadcastData {
  origin: CollabOrigin,
  object_id: String,
  /// "The payload is encoded using the `EncoderV1` with the `Message` struct.
  /// It can be parsed into: Message::Sync::(SyncMessage::Update(update))
  payload: Bytes,
}

impl CollabBroadcastData {
  pub fn new(origin: CollabOrigin, object_id: String, payload: Vec<u8>) -> Self {
    Self {
      origin,
      object_id,
      payload: Bytes::from(payload),
    }
  }
}

impl Display for CollabBroadcastData {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "server broadcast: [{}|oid:{}|len:{}]",
      self.origin,
      self.object_id,
      self.payload.len(),
    ))
  }
}

impl From<CollabBroadcastData> for CollabMessage {
  fn from(value: CollabBroadcastData) -> Self {
    CollabMessage::ServerBroadcast(value)
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

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct CloseCollabData {
  origin: CollabOrigin,
  object_id: String,
}

impl From<CollabMessage> for RealtimeMessage {
  fn from(msg: CollabMessage) -> Self {
    Self::Collab(msg)
  }
}
