use crate::pb;
use crate::pb::collab_message::Data;
use crate::pb::message::Payload;
use crate::pb::{message, SyncRequest};
use crate::shared::{Error, ObjectId, Rid, UpdateFlags};
use collab::preclude::sync::AwarenessUpdate;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{StateVector, Update};
use collab_entity::CollabType;
use prost::Message;
use std::fmt::{Debug, Formatter};
use uuid::Uuid;

/// Represents messages sent from the client to the server through the WebSocket connection.
/// ClientMessage is used to synchronize collaborative data between clients.
#[derive(Clone)]
pub enum ClientMessage {
  /// Requests synchronization by providing the client's state vector.
  /// The server responds with updates the client is missing.
  ///
  /// # Fields
  /// * `object_id` - The unique identifier of the collaborative object
  /// * `collab_type` - The type of collaborative object (document, folder, etc.)
  /// * `last_message_id` - The ID of the last message received by this client
  /// * `state_vector` - A compressed representation of the client's document state
  Manifest {
    object_id: ObjectId,
    collab_type: CollabType,
    last_message_id: Rid,
    state_vector: Vec<u8>,
  },

  /// Sends local changes to be synchronized with other clients.
  ///
  /// # Fields
  /// * `object_id` - The unique identifier of the collaborative object
  /// * `collab_type` - The type of collaborative object
  /// * `flags` - Encoding version flag (Lib0v1 or Lib0v2)
  /// * `update` - The encoded changes to be applied
  Update {
    object_id: ObjectId,
    collab_type: CollabType,
    flags: UpdateFlags,
    update: Vec<u8>,
  },

  /// Shares user presence and status information with other clients.
  ///
  /// # Fields
  /// * `object_id` - The unique identifier of the collaborative object
  /// * `collab_type` - The type of collaborative object
  /// * `awareness` - Encoded user presence data
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
  /// Returns a reference to the object ID contained in this message.
  pub fn object_id(&self) -> &ObjectId {
    match self {
      ClientMessage::Manifest { object_id, .. } => object_id,
      ClientMessage::Update { object_id, .. } => object_id,
      ClientMessage::AwarenessUpdate { object_id, .. } => object_id,
    }
  }

  /// Converts this ClientMessage into a serialized byte array.
  ///
  /// This is typically used before sending the message over the network.
  pub fn into_bytes(self) -> Result<Vec<u8>, Error> {
    Ok(pb::Message::from(self).encode_to_vec())
  }

  /// Creates a ClientMessage from a serialized byte array.
  ///
  /// This is typically used after receiving a message from the network.
  pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
    let proto = pb::Message::decode(bytes)?;
    Self::try_from(proto)
  }
}

/// Converts a ClientMessage into the protocol buffer message format.
/// This is used for serialization before network transmission.
impl From<ClientMessage> for pb::Message {
  fn from(value: ClientMessage) -> Self {
    match value {
      ClientMessage::Manifest {
        object_id,
        collab_type,
        last_message_id,
        state_vector,
      } => pb::Message {
        payload: Some(message::Payload::CollabMessage(pb::CollabMessage {
          object_id: object_id.to_string(),
          collab_type: collab_type as i32,
          data: Some(Data::SyncRequest(SyncRequest {
            last_message_id: Some(pb::Rid {
              timestamp: last_message_id.timestamp,
              counter: last_message_id.seq_no as u32,
            }),
            state_vector,
          })),
        })),
      },
      ClientMessage::Update {
        object_id,
        collab_type,
        flags,
        update,
      } => pb::Message {
        payload: Some(message::Payload::CollabMessage(pb::CollabMessage {
          object_id: object_id.to_string(),
          collab_type: collab_type as i32,
          data: Some(Data::Update(pb::Update {
            message_id: None,
            flags: flags as u8 as u32,
            payload: update,
          })),
        })),
      },
      ClientMessage::AwarenessUpdate {
        object_id,
        collab_type,
        awareness,
      } => {
        //
        pb::Message {
          payload: Some(message::Payload::CollabMessage(pb::CollabMessage {
            object_id: object_id.to_string(),
            collab_type: collab_type as i32,
            data: Some(Data::AwarenessUpdate(pb::AwarenessUpdate {
              payload: awareness,
            })),
          })),
        }
      },
    }
  }
}

/// Attempts to convert a protocol buffer message into a ClientMessage.
/// This is used for deserialization after receiving a message from the network.
impl TryFrom<pb::Message> for ClientMessage {
  type Error = Error;

  fn try_from(value: pb::Message) -> Result<Self, Self::Error> {
    match value.payload {
      None => Err(Error::MissingFields),
      Some(payload) => match payload {
        Payload::CollabMessage(value) => {
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
        },
        Payload::UserMessage(_) => Err(Error::UnsupportedClientMessage),
      },
    }
  }
}
