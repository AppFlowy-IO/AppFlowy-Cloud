use crate::pb;
use crate::pb::collab_message::Data;
use crate::pb::message::Payload;
use crate::pb::notification::{PermissionChanged, UserProfileChange};
#[rustfmt::skip]
use crate::pb::{SyncRequest, message};
use crate::shared::{Error, ObjectId, Rid, UpdateFlags};
use bytes::Bytes;
use collab::preclude::sync::AwarenessUpdate;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{StateVector, Update};
use collab_entity::CollabType;
use pb::notification::workspace_notification::Payload as NotificationPayload;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter};
use uuid::Uuid;

#[derive(Clone, Debug, serde_repr::Serialize_repr, serde_repr::Deserialize_repr)]
#[repr(u8)]
pub enum AccessChangedReason {
  PermissionDenied = 0,
  ObjectDeleted = 1,
}

impl Display for AccessChangedReason {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      AccessChangedReason::PermissionDenied => write!(f, "PermissionDenied"),
      AccessChangedReason::ObjectDeleted => write!(f, "ObjectDeleted"),
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
    reason: AccessChangedReason,
  },
  Notification {
    notification: WorkspaceNotification,
  },
}

impl ServerMessage {
  pub fn into_bytes(self) -> Result<Vec<u8>, Error> {
    Ok(pb::Message::from(self).encode_to_vec())
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
    let proto = pb::Message::decode(bytes)?;
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
      ServerMessage::Notification { notification } => f
        .debug_struct("WorkspaceNotification")
        .field("notification", &notification)
        .finish(),
    }
  }
}

impl From<ServerMessage> for pb::Message {
  fn from(value: ServerMessage) -> Self {
    match value {
      ServerMessage::Manifest {
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
      ServerMessage::Update {
        object_id,
        collab_type,
        flags,
        last_message_id,
        update,
      } => pb::Message {
        payload: Some(message::Payload::CollabMessage(pb::CollabMessage {
          object_id: object_id.to_string(),
          collab_type: collab_type as i32,
          data: Some(Data::Update(pb::Update {
            flags: flags as u8 as u32,
            message_id: Some(pb::Rid {
              timestamp: last_message_id.timestamp,
              counter: last_message_id.seq_no as u32,
            }),
            payload: update.into(),
          })),
        })),
      },
      ServerMessage::AwarenessUpdate {
        object_id,
        collab_type,
        awareness,
      } => pb::Message {
        payload: Some(message::Payload::CollabMessage(pb::CollabMessage {
          object_id: object_id.to_string(),
          collab_type: collab_type as i32,
          data: Some(Data::AwarenessUpdate(pb::AwarenessUpdate {
            payload: awareness.into(),
          })),
        })),
      },
      ServerMessage::AccessChanges {
        object_id,
        collab_type,
        can_read,
        can_write,
        reason,
      } => pb::Message {
        payload: Some(message::Payload::CollabMessage(pb::CollabMessage {
          object_id: object_id.to_string(),
          collab_type: collab_type as i32,
          data: Some(Data::AccessChanged(pb::AccessChanged {
            can_read,
            can_write,
            reason: reason as i32,
          })),
        })),
      },
      ServerMessage::Notification { notification } => match notification {
        WorkspaceNotification::UserProfileChange { uid, email, name } => pb::Message {
          payload: Some(message::Payload::Notification(
            pb::notification::WorkspaceNotification {
              payload: Some(NotificationPayload::ProfileChange(UserProfileChange {
                uid,
                name,
                email,
              })),
            },
          )),
        },
        WorkspaceNotification::ObjectAccessChanged { object_id, reason } => pb::Message {
          payload: Some(message::Payload::Notification(
            pb::notification::WorkspaceNotification {
              payload: Some(NotificationPayload::PermissionChanged(PermissionChanged {
                object_id: object_id.to_string(),
                reason: reason as u32,
              })),
            },
          )),
        },
      },
    }
  }
}

impl TryFrom<pb::Message> for ServerMessage {
  type Error = Error;

  fn try_from(value: pb::Message) -> Result<Self, Self::Error> {
    match value.payload {
      None => Err(Error::MissingFields),
      Some(payload) => match payload {
        Payload::CollabMessage(value) => {
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
                flags: UpdateFlags::try_from(proto.flags as u8)
                  .map_err(|_| Error::MissingFields)?,
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
              reason: AccessChangedReason::from(proto.reason),
            }),
            _ => Err(Error::MissingFields),
          }
        },
        Payload::Notification(notification) => match notification.payload {
          None => Err(Error::MissingFields),
          Some(payload) => match payload {
            NotificationPayload::ProfileChange(value) => Ok(ServerMessage::Notification {
              notification: WorkspaceNotification::UserProfileChange {
                uid: value.uid,
                email: value.email,
                name: value.name,
              },
            }),
            NotificationPayload::PermissionChanged(value) => {
              let object_id = Uuid::parse_str(&value.object_id)?;
              Ok(ServerMessage::Notification {
                notification: WorkspaceNotification::ObjectAccessChanged {
                  object_id,
                  reason: value.reason.into(),
                },
              })
            },
          },
        },
      },
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkspaceNotification {
  UserProfileChange {
    uid: i64,
    email: Option<String>,
    name: Option<String>,
  },
  ObjectAccessChanged {
    object_id: Uuid,
    reason: AccessChangedReason,
  },
}

impl From<AccessChangedReason> for i32 {
  fn from(value: AccessChangedReason) -> Self {
    value as i32
  }
}

impl From<i32> for AccessChangedReason {
  fn from(value: i32) -> Self {
    match value {
      0 => AccessChangedReason::PermissionDenied,
      1 => AccessChangedReason::ObjectDeleted,
      _ => AccessChangedReason::PermissionDenied,
    }
  }
}

impl From<u32> for AccessChangedReason {
  fn from(value: u32) -> Self {
    match value {
      0 => AccessChangedReason::PermissionDenied,
      1 => AccessChangedReason::ObjectDeleted,
      _ => AccessChangedReason::PermissionDenied,
    }
  }
}
