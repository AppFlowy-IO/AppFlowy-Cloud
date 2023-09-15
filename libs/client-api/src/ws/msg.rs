use crate::ws::WSError;
use collab_sync_protocol::CollabMessage;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Message;

pub type BusinessID = u8;

/// The message sent through WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeMessage {
  pub business_id: BusinessID,
  pub object_id: String,
  pub payload: Vec<u8>,
}

impl RealtimeMessage {
  pub fn new(business_id: BusinessID, object_id: String, payload: Vec<u8>) -> Self {
    Self {
      business_id,
      object_id,
      payload,
    }
  }
}

impl TryFrom<&[u8]> for RealtimeMessage {
  type Error = WSError;

  fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
    let msg = serde_json::from_slice::<RealtimeMessage>(bytes)?;
    Ok(msg)
  }
}

impl TryFrom<Vec<u8>> for RealtimeMessage {
  type Error = WSError;

  fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
    let msg = serde_json::from_slice::<RealtimeMessage>(&bytes)?;
    Ok(msg)
  }
}

impl TryFrom<&Message> for RealtimeMessage {
  type Error = WSError;

  fn try_from(value: &Message) -> Result<Self, Self::Error> {
    match value {
      Message::Binary(bytes) => {
        let msg = serde_json::from_slice::<RealtimeMessage>(bytes)?;
        Ok(msg)
      },
      _ => Err(WSError::UnsupportedMsgType),
    }
  }
}

impl From<RealtimeMessage> for Message {
  fn from(msg: RealtimeMessage) -> Self {
    let bytes = serde_json::to_vec(&msg).unwrap_or_default();
    Message::Binary(bytes)
  }
}

impl From<CollabMessage> for RealtimeMessage {
  fn from(msg: CollabMessage) -> Self {
    let business_id = msg.business_id();
    let object_id = msg.object_id().to_string();
    let payload = msg.to_vec();
    Self {
      business_id,
      object_id,
      payload,
    }
  }
}

impl TryFrom<RealtimeMessage> for CollabMessage {
  type Error = WSError;

  fn try_from(value: RealtimeMessage) -> Result<Self, Self::Error> {
    let msg =
      CollabMessage::from_vec(&value.payload).map_err(|e| WSError::Internal(Box::new(e)))?;
    Ok(msg)
  }
}

impl From<RealtimeMessage> for Result<CollabMessage, WSError> {
  fn from(msg: RealtimeMessage) -> Self {
    CollabMessage::try_from(msg)
  }
}
