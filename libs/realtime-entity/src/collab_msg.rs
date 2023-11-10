use anyhow::{anyhow, Error};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};

use crate::message::RealtimeMessage;
use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use collab::preclude::merge_updates_v1;
use collab::preclude::updates::decoder::DecoderV1;
use collab::preclude::updates::encoder::{Encode, Encoder, EncoderV1};
use collab::sync_protocol::message::{Message, MessageReader, SyncMessage};
use collab_entity::CollabType;
use serde::{Deserialize, Serialize};

pub trait CollabSinkMessage: Clone + Send + Sync + 'static + Ord + Display {
  fn collab_object_id(&self) -> &str;
  /// Returns the length of the message in bytes.
  fn length(&self) -> usize;

  /// Returns true if the message can be merged with other messages.
  fn can_merge(&self) -> bool;

  fn merge(&mut self, other: &Self, maximum_payload_size: &usize) -> Result<bool, Error>;

  fn is_init_msg(&self) -> bool;
  fn is_update_msg(&self) -> bool;

  /// Determine if the message can be deferred base on the current state of the sink.
  fn deferrable(&self) -> bool;
}

pub type MsgId = u64;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CollabMessage {
  ClientInit(ClientCollabInit),
  ClientUpdateSync(UpdateSync),
  ClientUpdateAck(UpdateAck),
  ServerInit(ServerCollabInit),
  ServerAwareness(CollabAwarenessData),
  ServerBroadcast(CollabBroadcastData),
  CloseCollab(CloseCollabData),
}

impl CollabSinkMessage for CollabMessage {
  fn collab_object_id(&self) -> &str {
    self.object_id()
  }

  fn length(&self) -> usize {
    self.len()
  }

  fn can_merge(&self) -> bool {
    self.is_update_msg()
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
    matches!(self, CollabMessage::ClientInit(_))
  }

  fn is_update_msg(&self) -> bool {
    matches!(self, CollabMessage::ClientUpdateSync(_))
  }

  fn deferrable(&self) -> bool {
    // If the message is not init, it can be pending.
    !matches!(self, CollabMessage::ClientInit(_))
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
      (CollabMessage::ClientInit { .. }, CollabMessage::ClientInit { .. }) => Ordering::Equal,
      (CollabMessage::ClientInit { .. }, _) => Ordering::Greater,
      (_, CollabMessage::ClientInit { .. }) => Ordering::Less,
      (CollabMessage::ServerInit(_), CollabMessage::ServerInit(_)) => Ordering::Equal,
      (CollabMessage::ServerInit { .. }, _) => Ordering::Greater,
      (_, CollabMessage::ServerInit { .. }) => Ordering::Less,
      _ => self.msg_id().cmp(&other.msg_id()).reverse(),
    }
  }
}

impl CollabMessage {
  /// Currently, only have one business id. So just return 1.
  pub fn business_id(&self) -> u8 {
    1
  }

  pub fn is_init(&self) -> bool {
    matches!(self, CollabMessage::ClientInit(_))
  }

  pub fn msg_id(&self) -> Option<MsgId> {
    match self {
      CollabMessage::ClientInit(value) => Some(value.msg_id),
      CollabMessage::ClientUpdateSync(value) => Some(value.msg_id),
      CollabMessage::ClientUpdateAck(value) => Some(value.msg_id),
      CollabMessage::ServerInit(value) => Some(value.msg_id),
      CollabMessage::ServerBroadcast(_) => None,
      CollabMessage::ServerAwareness(_) => None,
      CollabMessage::CloseCollab(_) => None,
    }
  }

  pub fn to_vec(&self) -> Vec<u8> {
    serde_json::to_vec(self).unwrap_or_default()
  }

  pub fn from_vec(data: &[u8]) -> Result<Self, serde_json::Error> {
    serde_json::from_slice(data)
  }

  pub fn len(&self) -> usize {
    self
      .payload()
      .map(|payload| payload.len())
      .unwrap_or_default()
  }

  pub fn payload(&self) -> Option<&Bytes> {
    match self {
      CollabMessage::ClientInit(value) => Some(&value.payload),
      CollabMessage::ClientUpdateSync(value) => Some(&value.payload),
      CollabMessage::ClientUpdateAck(value) => Some(&value.payload),
      CollabMessage::ServerInit(value) => Some(&value.payload),
      CollabMessage::ServerBroadcast(value) => Some(&value.payload),
      CollabMessage::ServerAwareness(value) => Some(&value.payload),
      CollabMessage::CloseCollab(_) => None,
    }
  }

  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }

  pub fn origin(&self) -> Option<&CollabOrigin> {
    match self {
      CollabMessage::ClientInit(value) => Some(&value.origin),
      CollabMessage::ClientUpdateSync(value) => Some(&value.origin),
      CollabMessage::ClientUpdateAck(value) => Some(&value.origin),
      CollabMessage::ServerInit(value) => Some(&value.origin),
      CollabMessage::ServerBroadcast(value) => Some(&value.origin),
      CollabMessage::ServerAwareness(_) => None,
      CollabMessage::CloseCollab(value) => Some(&value.origin),
    }
  }

  pub fn uid(&self) -> Option<i64> {
    self.origin().and_then(|origin| origin.client_user_id())
  }

  pub fn object_id(&self) -> &str {
    match self {
      CollabMessage::ClientInit(value) => &value.object_id,
      CollabMessage::ClientUpdateSync(value) => &value.object_id,
      CollabMessage::ClientUpdateAck(value) => &value.object_id,
      CollabMessage::ServerInit(value) => &value.object_id,
      CollabMessage::ServerBroadcast(value) => &value.object_id,
      CollabMessage::ServerAwareness(value) => &value.object_id,
      CollabMessage::CloseCollab(value) => &value.object_id,
    }
  }
}

impl Display for CollabMessage {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      CollabMessage::ClientInit(value) => f.write_fmt(format_args!(
        "client init: [{}|oid:{}|msg_id:{}|len:{}]",
        value.origin,
        value.object_id,
        value.msg_id,
        value.payload.len(),
      )),
      CollabMessage::ClientUpdateSync(value) => f.write_fmt(format_args!(
        "client update: [oid:{}|msg_id:{:?}|len:{}]",
        value.object_id,
        value.msg_id,
        value.payload.len(),
      )),
      CollabMessage::ClientUpdateAck(value) => f.write_fmt(format_args!(
        "ack: [oid:{}|msg_id:{:?}|len:{}]",
        value.object_id,
        value.msg_id,
        value.payload.len(),
      )),
      CollabMessage::ServerInit(value) => f.write_fmt(format_args!(
        "server init: [oid:{}|msg_id:{:?}|len:{}]",
        value.object_id,
        value.msg_id,
        value.payload.len(),
      )),
      CollabMessage::ServerBroadcast(value) => f.write_fmt(format_args!(
        "server broadcast: [{}|oid:{}|len:{}]",
        value.origin,
        value.object_id,
        value.payload.len(),
      )),
      CollabMessage::ServerAwareness(value) => f.write_fmt(format_args!(
        "awareness: [oid:{}|len:{}]",
        value.object_id,
        value.payload.len(),
      )),
      CollabMessage::CloseCollab(value) => {
        f.write_fmt(format_args!("close collab: [oid:{}]", value.object_id,))
      },
    }
  }
}

impl From<CollabMessage> for Bytes {
  fn from(msg: CollabMessage) -> Self {
    Bytes::from(msg.to_vec())
  }
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct CollabAwarenessData {
  object_id: String,
  payload: Bytes,
}

impl CollabAwarenessData {
  pub fn new(object_id: String, payload: Vec<u8>) -> Self {
    Self {
      object_id,
      payload: Bytes::from(payload),
    }
  }
}

impl From<CollabAwarenessData> for CollabMessage {
  fn from(value: CollabAwarenessData) -> Self {
    CollabMessage::ServerAwareness(value)
  }
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct ClientCollabInit {
  pub origin: CollabOrigin,
  pub object_id: String,
  pub collab_type: CollabType,
  pub workspace_id: String,
  pub msg_id: MsgId,
  pub payload: Bytes,
}

impl ClientCollabInit {
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

impl From<ClientCollabInit> for CollabMessage {
  fn from(value: ClientCollabInit) -> Self {
    CollabMessage::ClientInit(value)
  }
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct ServerCollabInit {
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

impl ServerCollabInit {
  pub fn new(origin: CollabOrigin, object_id: String, payload: Vec<u8>, msg_id: MsgId) -> Self {
    Self {
      origin,
      object_id,
      payload: Bytes::from(payload),
      msg_id,
    }
  }
}

impl From<ServerCollabInit> for CollabMessage {
  fn from(value: ServerCollabInit) -> Self {
    CollabMessage::ServerInit(value)
  }
}

impl Display for ServerCollabInit {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "server init: [uid:{:?}|oid:{}|msg_id:{:?}|len:{}]",
      self.origin.client_user_id(),
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
      let mut encoder = EncoderV1::new();
      let update = merge_updates_v1(&[&left, &right])?;
      let msg = Message::Sync(SyncMessage::Update(update));
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
      "client update sync: [uid:{:?}|oid:{}|msg_id:{:?}|len:{}]",
      self.origin.client_user_id(),
      self.object_id,
      self.msg_id,
      self.payload.len(),
    ))
  }
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct UpdateAck {
  pub origin: CollabOrigin,
  pub object_id: String,
  pub msg_id: MsgId,
  pub payload: Bytes,
}

impl UpdateAck {
  pub fn new(origin: CollabOrigin, object_id: String, payload: Vec<u8>, msg_id: MsgId) -> Self {
    Self {
      origin,
      object_id,
      payload: Bytes::from(payload),
      msg_id,
    }
  }
}

impl From<UpdateAck> for CollabMessage {
  fn from(value: UpdateAck) -> Self {
    CollabMessage::ClientUpdateAck(value)
  }
}

impl Display for UpdateAck {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "client update ack: [uid:{:?}|oid:{}|msg_id:{:?}|len:{}]",
      self.origin.client_user_id(),
      self.object_id,
      self.msg_id,
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

impl From<CloseCollabData> for CollabMessage {
  fn from(value: CloseCollabData) -> Self {
    CollabMessage::CloseCollab(value)
  }
}
impl From<CollabMessage> for RealtimeMessage {
  fn from(msg: CollabMessage) -> Self {
    Self::Collab(msg)
  }
}
