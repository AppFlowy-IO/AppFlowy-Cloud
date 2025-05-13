pub mod messages {
  include!(concat!(env!("OUT_DIR"), "/af_proto.messages.rs"));
}

use crate::messages::message::Data;
use crate::messages::SyncRequest;
use bytes::Bytes;
use collab::preclude::sync::AwarenessUpdate;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{StateVector, Update};
use collab_entity::CollabType;
use prost::Message;
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use uuid::Uuid;

pub type WorkspaceId = Uuid;
pub type ObjectId = Uuid;

/// Redis stream message ID, parsed.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Default, Hash)]
pub struct Rid {
  pub timestamp: u64,
  pub seq_no: u16,
}

impl Rid {
  pub fn new(timestamp: u64, seq_no: u16) -> Self {
    Rid { timestamp, seq_no }
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
    if bytes.len() != 10 {
      return Err(Error::InvalidRid);
    }
    let timestamp = u64::from_be_bytes([
      bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]);
    let seq_no = u16::from_be_bytes([bytes[8], bytes[9]]);
    Ok(Rid { timestamp, seq_no })
  }

  pub fn into_bytes(&self) -> [u8; 10] {
    let mut bytes = [0; 10];
    bytes[0..8].copy_from_slice(&self.timestamp.to_be_bytes());
    bytes[8..10].copy_from_slice(&self.seq_no.to_be_bytes());
    bytes
  }
}

impl Display for Rid {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}-{}", self.timestamp, self.seq_no)
  }
}

impl FromStr for Rid {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let mut parts = s.split('-');
    let timestamp = parts
      .next()
      .ok_or("missing timestamp")?
      .parse()
      .map_err(|e| format!("{}", e))?;
    let seq_no = parts
      .next()
      .ok_or("missing sequence number")?
      .parse()
      .map_err(|e| format!("{}", e))?;
    Ok(Rid { timestamp, seq_no })
  }
}

#[derive(Clone)]
pub enum ClientMessage {
  Manifest {
    object_id: ObjectId,
    collab_type: CollabType,
    last_message_id: Rid,
    state_vector: Vec<u8>,
  },
  Update {
    object_id: ObjectId,
    collab_type: CollabType,
    flags: UpdateFlags,
    update: Vec<u8>,
  },
  AwarenessUpdate {
    object_id: ObjectId,
    collab_type: CollabType,
    awareness: Vec<u8>,
  },
}

impl Debug for ClientMessage {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      ClientMessage::Manifest {
        object_id,
        collab_type,
        last_message_id,
        state_vector,
      } => {
        let state_vector = StateVector::decode_v1(state_vector).map_err(|_| std::fmt::Error)?;
        f.debug_struct("Manifest")
          .field("object_id", &object_id)
          .field("collab_type", &collab_type)
          .field("last_message_id", &last_message_id)
          .field("state_vector", &state_vector)
          .finish()
      },
      ClientMessage::Update {
        object_id,
        collab_type,
        flags,
        update,
      } => {
        let update = match flags {
          UpdateFlags::Lib0v1 => Update::decode_v1(update),
          UpdateFlags::Lib0v2 => Update::decode_v2(update),
        }
        .map_err(|_| std::fmt::Error)?;

        f.debug_struct("Update")
          .field("object_id", &object_id)
          .field("collab_type", &collab_type)
          .field("flags", &flags)
          .field("update", &update)
          .finish()
      },
      ClientMessage::AwarenessUpdate {
        object_id,
        collab_type,
        awareness,
      } => {
        let awareness = AwarenessUpdate::decode_v1(awareness).map_err(|_| std::fmt::Error)?;
        f.debug_struct("AwarenessUpdate")
          .field("object_id", &object_id)
          .field("collab_type", &collab_type)
          .field("awareness", &awareness)
          .finish()
      },
    }
  }
}

impl ClientMessage {
  pub fn object_id(&self) -> &ObjectId {
    match self {
      ClientMessage::Manifest { object_id, .. } => object_id,
      ClientMessage::Update { object_id, .. } => object_id,
      ClientMessage::AwarenessUpdate { object_id, .. } => object_id,
    }
  }

  pub fn collab_type(&self) -> Option<&CollabType> {
    match self {
      ClientMessage::Manifest { collab_type, .. } => Some(collab_type),
      ClientMessage::Update { collab_type, .. } => Some(collab_type),
      ClientMessage::AwarenessUpdate { .. } => None,
    }
  }

  pub fn into_bytes(self) -> Result<Vec<u8>, Error> {
    Ok(messages::Message::from(self).encode_to_vec())
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
    let proto = messages::Message::decode(bytes)?;
    Self::try_from(proto)
  }
}

impl From<ClientMessage> for messages::Message {
  fn from(value: ClientMessage) -> Self {
    match value {
      ClientMessage::Manifest {
        object_id,
        collab_type,
        last_message_id,
        state_vector,
      } => messages::Message {
        object_id: object_id.to_string(),
        collab_type: collab_type as i32,
        data: Some(Data::SyncRequest(SyncRequest {
          last_message_id: Some(messages::Rid {
            timestamp: last_message_id.timestamp,
            counter: last_message_id.seq_no as u32,
          }),
          state_vector,
        })),
      },
      ClientMessage::Update {
        object_id,
        collab_type,
        flags,
        update,
      } => messages::Message {
        object_id: object_id.to_string(),
        collab_type: collab_type as i32,
        data: Some(Data::Update(messages::Update {
          message_id: None,
          flags: flags as u8 as u32,
          payload: update,
        })),
      },
      ClientMessage::AwarenessUpdate {
        object_id,
        collab_type,
        awareness,
      } => messages::Message {
        object_id: object_id.to_string(),
        collab_type: collab_type as i32,
        data: Some(Data::AwarenessUpdate(messages::AwarenessUpdate {
          payload: awareness,
        })),
      },
    }
  }
}

impl TryFrom<messages::Message> for ClientMessage {
  type Error = Error;

  fn try_from(value: messages::Message) -> Result<Self, Self::Error> {
    let object_id = Uuid::parse_str(&value.object_id)?;
    let collab_type = CollabType::from(value.collab_type);

    match value.data {
      Some(Data::SyncRequest(proto)) => Ok(ClientMessage::Manifest {
        object_id,
        collab_type,
        last_message_id: Rid {
          timestamp: proto
            .last_message_id
            .as_ref()
            .ok_or(Error::MissingFields)?
            .timestamp,
          seq_no: proto
            .last_message_id
            .as_ref()
            .ok_or(Error::MissingFields)?
            .counter as u16,
        },
        state_vector: proto.state_vector,
      }),
      Some(Data::Update(proto)) => Ok(ClientMessage::Update {
        object_id,
        collab_type,
        flags: UpdateFlags::try_from(proto.flags as u8)?,
        update: proto.payload,
      }),
      Some(Data::AwarenessUpdate(proto)) => Ok(ClientMessage::AwarenessUpdate {
        object_id,
        collab_type,
        awareness: proto.payload,
      }),
      _ => Err(Error::MissingFields),
    }
  }
}

#[derive(Clone)]
pub enum ServerMessage {
  Manifest {
    object_id: ObjectId,
    collab_type: CollabType,
    last_message_id: Rid,
    state_vector: Vec<u8>,
  },
  Update {
    object_id: ObjectId,
    collab_type: CollabType,
    flags: UpdateFlags,
    last_message_id: Rid,
    update: Bytes,
  },
  AwarenessUpdate {
    object_id: ObjectId,
    collab_type: CollabType,
    awareness: Bytes,
  },
  AccessChanges {
    object_id: ObjectId,
    collab_type: CollabType,
    can_read: bool,
    can_write: bool,
    reason: String,
  },
}

impl ServerMessage {
  pub fn object_id(&self) -> &ObjectId {
    match self {
      ServerMessage::Manifest { object_id, .. } => object_id,
      ServerMessage::Update { object_id, .. } => object_id,
      ServerMessage::AwarenessUpdate { object_id, .. } => object_id,
      ServerMessage::AccessChanges { object_id, .. } => object_id,
    }
  }

  pub fn into_bytes(self) -> Result<Vec<u8>, Error> {
    Ok(messages::Message::from(self).encode_to_vec())
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
    let proto = messages::Message::decode(bytes)?;
    Self::try_from(proto)
  }
}

impl Debug for ServerMessage {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      ServerMessage::Manifest {
        object_id,
        collab_type,
        last_message_id,
        state_vector,
      } => {
        let state_vector = StateVector::decode_v1(state_vector).map_err(|_| std::fmt::Error)?;
        f.debug_struct("Manifest")
          .field("object_id", &object_id)
          .field("collab_type", &collab_type)
          .field("last_message_id", &last_message_id)
          .field("state_vector", &state_vector)
          .finish()
      },
      ServerMessage::Update {
        object_id,
        collab_type,
        flags,
        last_message_id,
        update,
      } => {
        let update = match flags {
          UpdateFlags::Lib0v1 => Update::decode_v1(update),
          UpdateFlags::Lib0v2 => Update::decode_v2(update),
        }
        .map_err(|_| std::fmt::Error)?;

        f.debug_struct("Update")
          .field("object_id", &object_id)
          .field("collab_type", &collab_type)
          .field("flags", &flags)
          .field("last_message_id", &last_message_id)
          .field("update", &update)
          .finish()
      },
      ServerMessage::AwarenessUpdate {
        object_id,
        collab_type,
        awareness,
      } => {
        let awareness = AwarenessUpdate::decode_v1(awareness).map_err(|_| std::fmt::Error)?;
        f.debug_struct("AwarenessUpdate")
          .field("object_id", &object_id)
          .field("collab_type", &collab_type)
          .field("awareness", &awareness)
          .finish()
      },
      ServerMessage::AccessChanges {
        object_id,
        collab_type,
        can_read,
        can_write,
        reason,
      } => f
        .debug_struct("PermissionDenied")
        .field("object_id", &object_id)
        .field("collab_type", &collab_type)
        .field("can_read", &can_read)
        .field("can_write", &can_write)
        .field("reason", &reason)
        .finish(),
    }
  }
}

impl From<ServerMessage> for messages::Message {
  fn from(value: ServerMessage) -> Self {
    match value {
      ServerMessage::Manifest {
        object_id,
        collab_type,
        last_message_id,
        state_vector,
      } => messages::Message {
        object_id: object_id.to_string(),
        collab_type: collab_type as i32,
        data: Some(Data::SyncRequest(messages::SyncRequest {
          last_message_id: Some(messages::Rid {
            timestamp: last_message_id.timestamp,
            counter: last_message_id.seq_no as u32,
          }),
          state_vector: state_vector.clone(),
        })),
      },
      ServerMessage::Update {
        object_id,
        collab_type,
        flags,
        last_message_id,
        update,
      } => messages::Message {
        object_id: object_id.to_string(),
        collab_type: collab_type as i32,
        data: Some(Data::Update(messages::Update {
          flags: flags as u8 as u32,
          message_id: Some(messages::Rid {
            timestamp: last_message_id.timestamp,
            counter: last_message_id.seq_no as u32,
          }),
          payload: update.into(),
        })),
      },
      ServerMessage::AwarenessUpdate {
        object_id,
        collab_type,
        awareness,
      } => messages::Message {
        object_id: object_id.to_string(),
        collab_type: collab_type as i32,
        data: Some(Data::AwarenessUpdate(messages::AwarenessUpdate {
          payload: awareness.into(),
        })),
      },
      ServerMessage::AccessChanges {
        object_id,
        collab_type,
        can_read,
        can_write,
        reason,
      } => messages::Message {
        object_id: object_id.to_string(),
        collab_type: collab_type as i32,
        data: Some(Data::AccessChanged(messages::AccessChanged {
          can_read,
          can_write,
          reason,
        })),
      },
    }
  }
}

impl TryFrom<messages::Message> for ServerMessage {
  type Error = Error;

  fn try_from(value: messages::Message) -> Result<Self, Self::Error> {
    let object_id = Uuid::parse_str(&value.object_id)?;
    let collab_type = CollabType::from(value.collab_type);
    match value.data {
      Some(Data::SyncRequest(proto)) => {
        let rid = proto.last_message_id.ok_or(Error::MissingFields)?;
        Ok(ServerMessage::Manifest {
          object_id,
          collab_type,
          last_message_id: Rid {
            timestamp: rid.timestamp,
            seq_no: rid.counter as u16,
          },
          state_vector: proto.state_vector,
        })
      },
      Some(Data::Update(proto)) => {
        let rid = proto.message_id.ok_or(Error::MissingFields)?;
        Ok(ServerMessage::Update {
          object_id,
          collab_type,
          flags: UpdateFlags::try_from(proto.flags as u8).map_err(|_| Error::MissingFields)?,
          last_message_id: Rid {
            timestamp: rid.timestamp,
            seq_no: rid.counter as u16,
          },
          update: proto.payload.into(),
        })
      },
      Some(Data::AwarenessUpdate(proto)) => Ok(ServerMessage::AwarenessUpdate {
        object_id,
        collab_type,
        awareness: proto.payload.into(),
      }),
      Some(Data::AccessChanged(proto)) => Ok(ServerMessage::AccessChanges {
        object_id,
        collab_type,
        can_read: proto.can_read,
        can_write: proto.can_write,
        reason: proto.reason,
      }),
      _ => Err(Error::MissingFields),
    }
  }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum UpdateFlags {
  #[default]
  Lib0v1 = 0,
  Lib0v2 = 1,
}

impl TryFrom<u8> for UpdateFlags {
  type Error = Error;

  fn try_from(value: u8) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(UpdateFlags::Lib0v1),
      1 => Ok(UpdateFlags::Lib0v2),
      tag => Err(Error::UnsupportedFlag(tag)),
    }
  }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
  #[error("failed to decode message: {0}")]
  ProtobufDecode(#[from] prost::DecodeError),
  #[error("failed to encode message: {0}")]
  ProtobufEncode(#[from] prost::EncodeError),
  #[error("failed to decode object id: {0}")]
  InvalidObjectId(#[from] uuid::Error),
  #[error("failed to decode Redis stream message ID")]
  InvalidRid,
  #[error("failed to decode message: missing fields")]
  MissingFields,
  #[error("failed to decode message: unsupported flag for update: {0}")]
  UnsupportedFlag(u8),
  #[error("failed to decode message: unknown collab type: {0}")]
  UnknownCollabType(u8),
}
