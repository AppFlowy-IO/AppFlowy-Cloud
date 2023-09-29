use crate::ws::WSError;
use collab_define::collab_msg::CollabMessage;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Copy, Clone, Serialize_repr, Deserialize_repr, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum BusinessID {
  CollabId = 1,
}

/// The message sent through WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRealtimeMessage {
  pub business_id: BusinessID,
  pub object_id: String,
  pub payload: Vec<u8>,
}

impl ClientRealtimeMessage {
  pub fn new(business_id: BusinessID, object_id: String, payload: Vec<u8>) -> Self {
    Self {
      business_id,
      object_id,
      payload,
    }
  }
}

impl TryFrom<&[u8]> for ClientRealtimeMessage {
  type Error = WSError;

  fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
    let msg = serde_json::from_slice::<ClientRealtimeMessage>(bytes)?;
    Ok(msg)
  }
}

impl TryFrom<Vec<u8>> for ClientRealtimeMessage {
  type Error = WSError;

  fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
    let msg = serde_json::from_slice::<ClientRealtimeMessage>(&bytes)?;
    Ok(msg)
  }
}

impl TryFrom<&Message> for ClientRealtimeMessage {
  type Error = WSError;

  fn try_from(value: &Message) -> Result<Self, Self::Error> {
    match value {
      Message::Binary(bytes) => {
        let msg = serde_json::from_slice::<ClientRealtimeMessage>(bytes)?;
        Ok(msg)
      },
      _ => Err(WSError::UnsupportedMsgType),
    }
  }
}

impl From<ClientRealtimeMessage> for Message {
  fn from(msg: ClientRealtimeMessage) -> Self {
    let bytes = serde_json::to_vec(&msg).unwrap_or_default();
    Message::Binary(bytes)
  }
}

impl From<CollabMessage> for ClientRealtimeMessage {
  fn from(msg: CollabMessage) -> Self {
    let business_id = BusinessID::CollabId;
    let object_id = msg.object_id().to_string();
    let payload = msg.to_vec();
    Self {
      business_id,
      object_id,
      payload,
    }
  }
}

impl TryFrom<ClientRealtimeMessage> for CollabMessage {
  type Error = WSError;

  fn try_from(value: ClientRealtimeMessage) -> Result<Self, Self::Error> {
    let msg =
      CollabMessage::from_vec(&value.payload).map_err(|e| WSError::Internal(Box::new(e)))?;
    Ok(msg)
  }
}

impl From<ClientRealtimeMessage> for Result<CollabMessage, WSError> {
  fn from(msg: ClientRealtimeMessage) -> Self {
    CollabMessage::try_from(msg)
  }
}
