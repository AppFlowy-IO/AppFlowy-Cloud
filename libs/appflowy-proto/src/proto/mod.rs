pub mod messages {
  include!(concat!(env!("OUT_DIR"), "/af_proto.messages.rs"));
}

use bytes::Bytes;
use prost::Message;
use std::fmt::{Display, Formatter};
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

#[derive(Debug, Clone)]
pub enum ClientMessage {
  Manifest {
    object_id: ObjectId,
    last_message_id: Rid,
    state_vector: Vec<u8>,
  },
  Update {
    object_id: ObjectId,
    flags: UpdateFlags,
    update: Vec<u8>,
  },
  AwarenessUpdate {
    object_id: ObjectId,
    awareness: Vec<u8>,
  },
}

impl ClientMessage {
  pub fn object_id(&self) -> &ObjectId {
    match self {
      ClientMessage::Manifest { object_id, .. } => object_id,
      ClientMessage::Update { object_id, .. } => object_id,
      ClientMessage::AwarenessUpdate { object_id, .. } => object_id,
    }
  }

  pub fn into_bytes(self) -> Result<Bytes, Error> {
    Ok(messages::Message::from(self).encode_to_vec().into())
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
    let proto = messages::Message::decode(bytes)?;
    Self::try_from(proto)
  }
}

impl From<ClientMessage> for messages::Message {
  fn from(value: ClientMessage) -> Self {
    use messages::message::Message as ProtoMessage;
    match value {
      ClientMessage::Manifest {
        object_id,
        last_message_id,
        state_vector,
      } => crate::proto::messages::Message {
        message: Some(ProtoMessage::Manifest(messages::Manifest {
          object_id: object_id.as_bytes().to_vec(),
          last_message_id: Some(messages::Rid {
            timestamp: last_message_id.timestamp,
            counter: last_message_id.seq_no as u32,
          }),
          state_vector: state_vector.clone(),
        })),
      },
      ClientMessage::Update {
        object_id,
        flags,
        update,
      } => messages::Message {
        message: Some(ProtoMessage::Update(messages::Update {
          object_id: object_id.as_bytes().to_vec(),
          flags: flags as u8 as u32,
          message_id: None,
          payload: update,
        })),
      },
      ClientMessage::AwarenessUpdate {
        object_id,
        awareness,
      } => messages::Message {
        message: Some(ProtoMessage::AwarenessUpdate(messages::AwarenessUpdate {
          object_id: object_id.as_bytes().to_vec(),
          payload: awareness,
        })),
      },
    }
  }
}

impl TryFrom<messages::Message> for ClientMessage {
  type Error = Error;

  fn try_from(value: messages::Message) -> Result<Self, Self::Error> {
    use messages::message::Message as ProtoMessage;
    match value.message {
      Some(ProtoMessage::Manifest(proto)) => Ok(ClientMessage::Manifest {
        object_id: Uuid::from_slice(&proto.object_id).map_err(Error::InvalidObjectId)?,
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
      Some(ProtoMessage::Update(proto)) => Ok(ClientMessage::Update {
        object_id: Uuid::from_slice(&proto.object_id).map_err(Error::InvalidObjectId)?,
        flags: UpdateFlags::try_from(proto.flags as u8)?,
        update: proto.payload,
      }),
      Some(ProtoMessage::AwarenessUpdate(proto)) => Ok(ClientMessage::AwarenessUpdate {
        object_id: Uuid::from_slice(&proto.object_id).map_err(Error::InvalidObjectId)?,
        awareness: proto.payload,
      }),
      _ => Err(Error::MissingFields),
    }
  }
}

#[derive(Debug, Clone)]
pub enum ServerMessage {
  Manifest {
    object_id: ObjectId,
    last_message_id: Rid,
    state_vector: Vec<u8>,
  },
  Update {
    object_id: ObjectId,
    flags: UpdateFlags,
    last_message_id: Rid,
    update: Bytes,
  },
  AwarenessUpdate {
    object_id: ObjectId,
    awareness: Bytes,
  },
  PermissionDenied {
    object_id: ObjectId,
    reason: String,
  },
}

impl ServerMessage {
  pub fn object_id(&self) -> &ObjectId {
    match self {
      ServerMessage::Manifest { object_id, .. } => object_id,
      ServerMessage::Update { object_id, .. } => object_id,
      ServerMessage::AwarenessUpdate { object_id, .. } => object_id,
      ServerMessage::PermissionDenied { object_id, .. } => object_id,
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

impl From<ServerMessage> for messages::Message {
  fn from(value: ServerMessage) -> Self {
    use messages::message::Message as ProtoMessage;
    match value {
      ServerMessage::Manifest {
        object_id,
        last_message_id,
        state_vector,
      } => messages::Message {
        message: Some(ProtoMessage::Manifest(messages::Manifest {
          object_id: object_id.as_bytes().to_vec(),
          last_message_id: Some(messages::Rid {
            timestamp: last_message_id.timestamp,
            counter: last_message_id.seq_no as u32,
          }),
          state_vector: state_vector.clone(),
        })),
      },
      ServerMessage::Update {
        object_id,
        flags,
        last_message_id,
        update,
      } => messages::Message {
        message: Some(ProtoMessage::Update(messages::Update {
          object_id: object_id.as_bytes().to_vec(),
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
        awareness,
      } => messages::Message {
        message: Some(ProtoMessage::AwarenessUpdate(messages::AwarenessUpdate {
          object_id: object_id.as_bytes().to_vec(),
          payload: awareness.into(),
        })),
      },
      ServerMessage::PermissionDenied { object_id, reason } => messages::Message {
        message: Some(ProtoMessage::PermissionRevoked(
          messages::PermissionRevoked {
            object_id: object_id.as_bytes().to_vec(),
            reason,
          },
        )),
      },
    }
  }
}

impl TryFrom<messages::Message> for ServerMessage {
  type Error = Error;

  fn try_from(value: messages::Message) -> Result<Self, Self::Error> {
    use messages::message::Message as ProtoMessage;
    match value.message {
      Some(ProtoMessage::Manifest(proto)) => {
        let rid = proto.last_message_id.ok_or(Error::MissingFields)?;
        Ok(ServerMessage::Manifest {
          object_id: Uuid::from_slice(&proto.object_id).map_err(Error::InvalidObjectId)?,
          last_message_id: Rid {
            timestamp: rid.timestamp,
            seq_no: rid.counter as u16,
          },
          state_vector: proto.state_vector,
        })
      },
      Some(ProtoMessage::Update(proto)) => {
        let rid = proto.message_id.ok_or(Error::MissingFields)?;
        Ok(ServerMessage::Update {
          object_id: Uuid::from_slice(&proto.object_id).map_err(Error::InvalidObjectId)?,
          flags: UpdateFlags::try_from(proto.flags as u8).map_err(|_| Error::MissingFields)?,
          last_message_id: Rid {
            timestamp: rid.timestamp,
            seq_no: rid.counter as u16,
          },
          update: proto.payload.into(),
        })
      },
      Some(ProtoMessage::AwarenessUpdate(proto)) => Ok(ServerMessage::AwarenessUpdate {
        object_id: Uuid::from_slice(&proto.object_id).map_err(Error::InvalidObjectId)?,
        awareness: proto.payload.into(),
      }),
      Some(ProtoMessage::PermissionRevoked(proto)) => Ok(ServerMessage::PermissionDenied {
        object_id: Uuid::from_slice(&proto.object_id).map_err(Error::InvalidObjectId)?,
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
}
