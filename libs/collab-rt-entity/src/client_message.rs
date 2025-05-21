use crate::message::RealtimeMessage;
use crate::server_message::ServerInit;
use crate::{CollabMessage, MessageByObjectId, MsgId};
use anyhow::{anyhow, Error};
use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use collab_entity::CollabType;
use collab_rt_protocol::{Message, MessageReader, SyncMessage};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use yrs::merge_updates_v1;
use yrs::updates::decoder::DecoderV1;
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};

pub trait SinkMessage: Clone + Send + Sync + 'static + Ord + Display {
  fn payload_size(&self) -> usize;
  fn mergeable(&self) -> bool;
  fn merge(&mut self, other: &Self, maximum_payload_size: &usize) -> Result<bool, Error>;
  fn is_client_init_sync(&self) -> bool;
  fn is_server_init_sync(&self) -> bool;
  fn is_update_sync(&self) -> bool;
  fn is_awareness_sync(&self) -> bool;
  fn is_ping_sync(&self) -> bool;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientCollabMessage {
  ClientInitSync { data: InitSync },
  ClientUpdateSync { data: UpdateSync },
  ServerInitSync(ServerInit),
  ClientAwarenessSync(UpdateSync),
  ClientCollabStateCheck(CollabStateCheck),
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
      ClientCollabMessage::ClientCollabStateCheck(_) => 0,
    }
  }
  pub fn object_id(&self) -> &str {
    match self {
      ClientCollabMessage::ClientInitSync { data, .. } => &data.object_id,
      ClientCollabMessage::ClientUpdateSync { data, .. } => &data.object_id,
      ClientCollabMessage::ServerInitSync(msg) => &msg.object_id,
      ClientCollabMessage::ClientAwarenessSync(data) => &data.object_id,
      ClientCollabMessage::ClientCollabStateCheck(data) => &data.object_id,
    }
  }

  pub fn origin(&self) -> &CollabOrigin {
    match self {
      ClientCollabMessage::ClientInitSync { data, .. } => &data.origin,
      ClientCollabMessage::ClientUpdateSync { data, .. } => &data.origin,
      ClientCollabMessage::ServerInitSync(msg) => &msg.origin,
      ClientCollabMessage::ClientAwarenessSync(data) => &data.origin,
      ClientCollabMessage::ClientCollabStateCheck(data) => &data.origin,
    }
  }
  pub fn payload(&self) -> &Bytes {
    static EMPTY_BYTES: Bytes = Bytes::from_static(b"");
    match self {
      ClientCollabMessage::ClientInitSync { data, .. } => &data.payload,
      ClientCollabMessage::ClientUpdateSync { data, .. } => &data.payload,
      ClientCollabMessage::ServerInitSync(msg) => &msg.payload,
      ClientCollabMessage::ClientAwarenessSync(data) => &data.payload,
      ClientCollabMessage::ClientCollabStateCheck(_data) => &EMPTY_BYTES,
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
      ClientCollabMessage::ClientCollabStateCheck(data) => data.msg_id,
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
      ClientCollabMessage::ClientCollabStateCheck(data) => Display::fmt(data, f),
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
        "Can't convert to ClientCollabMessage for value:{}",
        value
      )),
    }
  }
}

impl From<ClientCollabMessage> for RealtimeMessage {
  fn from(msg: ClientCollabMessage) -> Self {
    let object_id = msg.object_id().to_string();
    let message = MessageByObjectId::new_with_message(object_id, vec![msg]);
    Self::ClientCollabV2(message)
  }
}

impl SinkMessage for ClientCollabMessage {
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

  fn is_awareness_sync(&self) -> bool {
    matches!(self, ClientCollabMessage::ClientAwarenessSync { .. })
  }

  fn is_ping_sync(&self) -> bool {
    matches!(self, ClientCollabMessage::ClientCollabStateCheck { .. })
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
      let update = merge_updates_v1([left, right])?;
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

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize, Hash)]
pub struct CollabStateCheck {
  pub origin: CollabOrigin,
  pub object_id: String,
  pub msg_id: MsgId,
}

impl Display for CollabStateCheck {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "ping: [uid:{}|oid:{}|msg_id:{:?}]",
      self.origin.client_user_id().unwrap_or(0),
      self.object_id,
      self.msg_id,
    ))
  }
}
