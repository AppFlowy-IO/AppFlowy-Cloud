use anyhow::{anyhow, Error};
use std::cmp::Ordering;

use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};

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
  fn payload_size(&self) -> usize;
  fn mergeable(&self) -> bool;

  fn merge(&mut self, other: &Self, maximum_payload_size: &usize) -> Result<bool, Error>;

  fn is_client_init_sync(&self) -> bool;
  fn is_server_init_sync(&self) -> bool;
  fn is_update_sync(&self) -> bool;

  fn set_msg_id(&mut self, msg_id: MsgId);

  fn get_msg_id(&self) -> Option<MsgId>;
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

impl CollabSinkMessage for CollabMessage {
  fn payload_size(&self) -> usize {
    self.len()
  }

  fn mergeable(&self) -> bool {
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

  fn is_client_init_sync(&self) -> bool {
    matches!(self, CollabMessage::ClientInitSync(_))
  }

  fn is_server_init_sync(&self) -> bool {
    matches!(self, CollabMessage::ServerInitSync(_))
  }

  fn is_update_sync(&self) -> bool {
    matches!(self, CollabMessage::ClientUpdateSync(_))
  }

  fn set_msg_id(&mut self, msg_id: MsgId) {
    match self {
      CollabMessage::ClientInitSync(value) => value.msg_id = msg_id,
      CollabMessage::ClientUpdateSync(value) => value.msg_id = msg_id,
      CollabMessage::ClientAck(value) => value.source.msg_id = msg_id,
      CollabMessage::ServerInitSync(value) => value.msg_id = msg_id,
      CollabMessage::ServerBroadcast(_) => {},
      CollabMessage::AwarenessSync(_) => {},
    }
  }

  fn get_msg_id(&self) -> Option<MsgId> {
    self.msg_id()
  }
}

impl Hash for CollabMessage {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.origin().hash(state);
    self.msg_id().hash(state);
    self.object_id().hash(state);
  }
}

impl Eq for CollabMessage {}

impl PartialEq for CollabMessage {
  fn eq(&self, other: &Self) -> bool {
    self.msg_id() == other.msg_id()
      && self.origin() == other.origin()
      && self.object_id() == other.object_id()
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

///  ⚠️ ⚠️ ⚠️Compatibility Warning:
///
/// The structure of this struct is integral to maintaining compatibility with existing messages.
/// Therefore, adding or removing any properties (fields) from this struct could disrupt the
/// compatibility. Such changes may lead to issues in processing existing messages that expect
/// the struct to have a specific format. It's crucial to carefully consider the implications
/// of modifying this struct's fields
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize, Hash)]
pub struct AwarenessSync {
  object_id: String,
  payload: Bytes,
  origin: CollabOrigin,
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

impl From<AwarenessSync> for CollabMessage {
  fn from(value: AwarenessSync) -> Self {
    CollabMessage::AwarenessSync(value)
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

///  ⚠️ ⚠️ ⚠️Compatibility Warning:
///
/// The structure of this struct is integral to maintaining compatibility with existing messages.
/// Therefore, adding or removing any properties (fields) from this struct could disrupt the
/// compatibility. Such changes may lead to issues in processing existing messages that expect
/// the struct to have a specific format. It's crucial to carefully consider the implications
/// of modifying this struct's fields

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize, Hash)]
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
      "client init: [uid:{}|oid:{}|msg_id:{}|len:{}]",
      self.origin.client_user_id().unwrap_or(0),
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

impl From<ServerInit> for CollabMessage {
  fn from(value: ServerInit) -> Self {
    CollabMessage::ServerInitSync(value)
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
      "update: [uid:{}|oid:{}|msg_id:{:?}|len:{}]",
      self.origin.client_user_id().unwrap_or(0),
      self.object_id,
      self.msg_id,
      self.payload.len(),
    ))
  }
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize_repr, Deserialize_repr, Hash)]
#[repr(u8)]
pub enum AckCode {
  Success = 0,
  CannotApplyUpdate = 1,
  Retry = 2,
  Internal = 3,
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
  pub source: AckSource,
  pub payload: Bytes,
  pub code: AckCode,
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize, Hash)]
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
      "ack: [uid:{}|oid:{}|msg_id:{:?}|len:{}]",
      self.origin.client_user_id().unwrap_or(0),
      self.object_id,
      self.source.msg_id,
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
  origin: CollabOrigin,
  object_id: String,
  /// "The payload is encoded using the `EncoderV1` with the `Message` struct.
  /// It can be parsed into: Message::Sync::(SyncMessage::Update(update))
  payload: Bytes,
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
      "broadcast: [uid:{}|oid:{}|len:{}]",
      self.origin.client_user_id().unwrap_or(0),
      self.object_id,
      self.payload.len(),
    ))
  }
}

impl From<BroadcastSync> for CollabMessage {
  fn from(value: BroadcastSync) -> Self {
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientCollabMessage {
  ClientInitSync { data: InitSync },
  ClientUpdateSync { data: UpdateSync },
  ServerInitSync(ServerInit),
  ClientAwarenessSync(UpdateSync),
}

impl ClientCollabMessage {
  pub fn new_init_sync(data: InitSync) -> Self {
    Self::ClientInitSync { data }
  }

  pub fn new_update_sync(data: UpdateSync) -> Self {
    Self::ClientUpdateSync { data }
  }

  pub fn new_server_init_sync(data: ServerInit) -> Self {
    Self::ServerInitSync(data)
  }

  pub fn new_awareness_sync(data: UpdateSync) -> Self {
    Self::ClientAwarenessSync(data)
  }
  pub fn size(&self) -> usize {
    match self {
      ClientCollabMessage::ClientInitSync { data, .. } => data.payload.len(),
      ClientCollabMessage::ClientUpdateSync { data, .. } => data.payload.len(),
      ClientCollabMessage::ServerInitSync(msg) => msg.payload.len(),
      ClientCollabMessage::ClientAwarenessSync(data) => data.payload.len(),
    }
  }
  pub fn object_id(&self) -> &str {
    match self {
      ClientCollabMessage::ClientInitSync { data, .. } => &data.object_id,
      ClientCollabMessage::ClientUpdateSync { data, .. } => &data.object_id,
      ClientCollabMessage::ServerInitSync(msg) => &msg.object_id,
      ClientCollabMessage::ClientAwarenessSync(data) => &data.object_id,
    }
  }

  pub fn origin(&self) -> &CollabOrigin {
    match self {
      ClientCollabMessage::ClientInitSync { data, .. } => &data.origin,
      ClientCollabMessage::ClientUpdateSync { data, .. } => &data.origin,
      ClientCollabMessage::ServerInitSync(msg) => &msg.origin,
      ClientCollabMessage::ClientAwarenessSync(data) => &data.origin,
    }
  }
  pub fn payload(&self) -> &Bytes {
    match self {
      ClientCollabMessage::ClientInitSync { data, .. } => &data.payload,
      ClientCollabMessage::ClientUpdateSync { data, .. } => &data.payload,
      ClientCollabMessage::ServerInitSync(msg) => &msg.payload,
      ClientCollabMessage::ClientAwarenessSync(data) => &data.payload,
    }
  }
  pub fn device_id(&self) -> Option<String> {
    match self.origin() {
      CollabOrigin::Client(origin) => Some(origin.device_id.clone()),
      _ => None,
    }
  }

  pub fn msg_id(&self) -> MsgId {
    match self {
      ClientCollabMessage::ClientInitSync { data, .. } => data.msg_id,
      ClientCollabMessage::ClientUpdateSync { data, .. } => data.msg_id,
      ClientCollabMessage::ServerInitSync(value) => value.msg_id,
      ClientCollabMessage::ClientAwarenessSync(data) => data.msg_id,
    }
  }

  pub fn is_init_sync(&self) -> bool {
    matches!(self, ClientCollabMessage::ClientInitSync { .. })
  }
}

impl Display for ClientCollabMessage {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      ClientCollabMessage::ClientInitSync { data, .. } => Display::fmt(&data, f),
      ClientCollabMessage::ClientUpdateSync { data, .. } => Display::fmt(&data, f),
      ClientCollabMessage::ServerInitSync(value) => Display::fmt(&value, f),
      ClientCollabMessage::ClientAwarenessSync(data) => f.write_fmt(format_args!(
        "awareness: [uid:{}|oid:{}|msg_id:{}|len:{}]",
        data.origin.client_user_id().unwrap_or(0),
        data.object_id,
        data.msg_id,
        data.payload.len(),
      )),
    }
  }
}

impl TryFrom<CollabMessage> for ClientCollabMessage {
  type Error = Error;

  fn try_from(value: CollabMessage) -> Result<Self, Self::Error> {
    match value {
      CollabMessage::ClientInitSync(msg) => Ok(ClientCollabMessage::ClientInitSync { data: msg }),
      CollabMessage::ClientUpdateSync(msg) => {
        Ok(ClientCollabMessage::ClientUpdateSync { data: msg })
      },
      CollabMessage::ServerInitSync(msg) => Ok(ClientCollabMessage::ServerInitSync(msg)),
      _ => Err(anyhow!(
        "Can't convert to ClientCollabMessage for given collab message:{}",
        value
      )),
    }
  }
}

impl From<ClientCollabMessage> for CollabMessage {
  fn from(value: ClientCollabMessage) -> Self {
    match value {
      ClientCollabMessage::ClientInitSync { data, .. } => CollabMessage::ClientInitSync(data),
      ClientCollabMessage::ClientUpdateSync { data, .. } => CollabMessage::ClientUpdateSync(data),
      ClientCollabMessage::ServerInitSync(data) => CollabMessage::ServerInitSync(data),
      ClientCollabMessage::ClientAwarenessSync(data) => CollabMessage::ClientUpdateSync(data),
    }
  }
}

impl From<ClientCollabMessage> for RealtimeMessage {
  fn from(msg: ClientCollabMessage) -> Self {
    let object_id = msg.object_id().to_string();
    Self::ClientCollabV2([(object_id, vec![msg])].into())
  }
}

impl CollabSinkMessage for ClientCollabMessage {
  fn payload_size(&self) -> usize {
    self.size()
  }

  fn mergeable(&self) -> bool {
    matches!(self, ClientCollabMessage::ClientUpdateSync { .. })
  }

  fn merge(&mut self, other: &Self, maximum_payload_size: &usize) -> Result<bool, Error> {
    match (self, other) {
      (
        ClientCollabMessage::ClientUpdateSync { data, .. },
        ClientCollabMessage::ClientUpdateSync { data: other, .. },
      ) => {
        if &data.payload.len() > maximum_payload_size {
          Ok(false)
        } else {
          data.merge_payload(other)
        }
      },
      _ => Ok(false),
    }
  }

  fn is_client_init_sync(&self) -> bool {
    matches!(self, ClientCollabMessage::ClientInitSync { .. })
  }

  fn is_server_init_sync(&self) -> bool {
    matches!(self, ClientCollabMessage::ServerInitSync { .. })
  }

  fn is_update_sync(&self) -> bool {
    matches!(self, ClientCollabMessage::ClientUpdateSync { .. })
  }

  fn set_msg_id(&mut self, msg_id: MsgId) {
    match self {
      ClientCollabMessage::ClientInitSync { data, .. } => data.msg_id = msg_id,
      ClientCollabMessage::ClientUpdateSync { data, .. } => data.msg_id = msg_id,
      ClientCollabMessage::ServerInitSync(data) => data.msg_id = msg_id,
      ClientCollabMessage::ClientAwarenessSync(data) => data.msg_id = msg_id,
    }
  }

  fn get_msg_id(&self) -> Option<MsgId> {
    Some(self.msg_id())
  }
}

impl Hash for ClientCollabMessage {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.origin().hash(state);
    self.msg_id().hash(state);
    self.object_id().hash(state);
  }
}

impl Eq for ClientCollabMessage {}

impl PartialEq for ClientCollabMessage {
  fn eq(&self, other: &Self) -> bool {
    self.msg_id() == other.msg_id()
      && self.object_id() == other.object_id()
      && self.origin() == other.origin()
  }
}

impl PartialOrd for ClientCollabMessage {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for ClientCollabMessage {
  fn cmp(&self, other: &Self) -> Ordering {
    match (&self, &other) {
      (ClientCollabMessage::ClientInitSync { .. }, ClientCollabMessage::ClientInitSync { .. }) => {
        Ordering::Equal
      },
      (ClientCollabMessage::ClientInitSync { .. }, _) => Ordering::Greater,
      (_, ClientCollabMessage::ClientInitSync { .. }) => Ordering::Less,
      (ClientCollabMessage::ServerInitSync(_left), ClientCollabMessage::ServerInitSync(_right)) => {
        Ordering::Equal
      },
      (ClientCollabMessage::ServerInitSync { .. }, _) => Ordering::Greater,
      (_, ClientCollabMessage::ServerInitSync { .. }) => Ordering::Less,
      _ => self.msg_id().cmp(&other.msg_id()).reverse(),
    }
  }
}

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
      ServerCollabMessage::ClientAck(value) => Some(value.source.msg_id),
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
