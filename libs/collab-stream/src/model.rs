use crate::error::{internal, StreamError};
use bytes::Bytes;
use collab_entity::proto::collab::collab_update_event::Update;
use collab_entity::{proto, CollabType};
use prost::Message;
use redis::streams::StreamId;
use redis::{FromRedisValue, RedisError, RedisResult, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use std::str::FromStr;

/// The [MessageId] generated by XADD has two parts: a timestamp and a sequence number, separated by
/// a hyphen (-). The timestamp is based on the server's time when the message is added, and the
/// sequence number is used to differentiate messages added at the same millisecond.
///
///  If multiple messages are added within the same millisecond, Redis increments the sequence number
/// for each subsequent message
///
/// An example message ID might look like this: 1631020452097-0. In this example, 1631020452097 is
/// the timestamp in milliseconds, and 0 is the sequence number.
#[derive(Debug, Clone)]
pub struct MessageId {
  pub timestamp_ms: u64,
  pub sequence_number: u16,
}

impl Display for MessageId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}-{}", self.timestamp_ms, self.sequence_number)
  }
}

impl TryFrom<&[u8]> for MessageId {
  type Error = StreamError;

  fn try_from(s: &[u8]) -> Result<Self, Self::Error> {
    let s = std::str::from_utf8(s)?;
    Self::try_from(s)
  }
}

impl TryFrom<&str> for MessageId {
  type Error = StreamError;

  fn try_from(s: &str) -> Result<Self, Self::Error> {
    let parts: Vec<_> = s.splitn(2, '-').collect();

    if parts.len() != 2 {
      return Err(StreamError::InvalidFormat);
    }

    // Directly parse without intermediate assignment.
    let timestamp_ms = u64::from_str(parts[0])?;
    let sequence_number = u16::from_str(parts[1])?;

    Ok(MessageId {
      timestamp_ms,
      sequence_number,
    })
  }
}

impl TryFrom<String> for MessageId {
  type Error = StreamError;

  fn try_from(s: String) -> Result<Self, Self::Error> {
    Self::try_from(s.as_str())
  }
}

impl FromRedisValue for MessageId {
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    match v {
      Value::Data(stream_key) => MessageId::try_from(stream_key.as_slice()).map_err(|_| {
        RedisError::from((
          redis::ErrorKind::TypeError,
          "invalid stream key",
          format!("{:?}", stream_key),
        ))
      }),
      _ => Err(internal("expecting Value::Data")),
    }
  }
}

#[derive(Debug)]
pub struct StreamMessageByStreamKey(pub BTreeMap<String, Vec<StreamMessage>>);

impl FromRedisValue for StreamMessageByStreamKey {
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    let mut map: BTreeMap<String, Vec<StreamMessage>> = BTreeMap::new();
    if matches!(v, Value::Nil) {
      return Ok(StreamMessageByStreamKey(map));
    }

    let value_by_id = bulk_from_redis_value(v)?.iter();
    for value in value_by_id {
      let key_values = bulk_from_redis_value(value)?;

      if key_values.len() != 2 {
        return Err(RedisError::from((
          redis::ErrorKind::TypeError,
          "Invalid length",
          "Expected length of 2 for the outer bulk value".to_string(),
        )));
      }

      let stream_key = RedisString::from_redis_value(&key_values[0])?.0;
      let values = bulk_from_redis_value(&key_values[1])?.iter();
      for value in values {
        let value = StreamMessage::from_redis_value(value)?;
        map.entry(stream_key.clone()).or_default().push(value);
      }
    }

    Ok(StreamMessageByStreamKey(map))
  }
}

/// A message in the Redis stream. It's the same as [StreamBinary] but with additional metadata.
#[derive(Debug, Clone)]
pub struct StreamMessage {
  pub data: Bytes,
  /// only applicable when reading from redis
  pub id: MessageId,
}

impl FromRedisValue for StreamMessage {
  // Optimized parsing function
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    let bulk = bulk_from_redis_value(v)?;
    if bulk.len() != 2 {
      return Err(RedisError::from((
        redis::ErrorKind::TypeError,
        "Invalid length",
        format!(
          "Expected length of 2 for the outer bulk value, but got:{}",
          bulk.len()
        ),
      )));
    }

    let id = MessageId::from_redis_value(&bulk[0])?;
    let fields = bulk_from_redis_value(&bulk[1])?;
    if fields.len() != 2 {
      return Err(RedisError::from((
        redis::ErrorKind::TypeError,
        "Invalid length",
        format!(
          "Expected length of 2 for the bulk value, but got {}",
          fields.len()
        ),
      )));
    }

    verify_field(&fields[0], "data")?;
    let raw_data = Vec::<u8>::from_redis_value(&fields[1])?;

    Ok(StreamMessage {
      data: Bytes::from(raw_data),
      id,
    })
  }
}

impl TryFrom<StreamId> for StreamMessage {
  type Error = StreamError;

  fn try_from(value: StreamId) -> Result<Self, Self::Error> {
    let id = MessageId::try_from(value.id.as_str())?;
    let data = value
      .get("data")
      .ok_or(StreamError::UnexpectedValue("data".to_string()))?;
    Ok(Self { data, id })
  }
}

#[derive(Debug)]
pub struct StreamBinary(pub Vec<u8>);

impl From<StreamMessage> for StreamBinary {
  fn from(m: StreamMessage) -> Self {
    Self(m.data.to_vec())
  }
}

impl Deref for StreamBinary {
  type Target = Vec<u8>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl StreamBinary {
  pub fn into_tuple_array(self) -> [(&'static str, Vec<u8>); 1] {
    static DATA: &str = "data";
    [(DATA, self.0)]
  }
}

impl TryFrom<Vec<u8>> for StreamBinary {
  type Error = StreamError;

  fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
    Ok(Self(value))
  }
}

impl TryFrom<&[u8]> for StreamBinary {
  type Error = StreamError;

  fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
    Ok(Self(value.to_vec()))
  }
}

fn verify_field(field: &Value, expected: &str) -> RedisResult<()> {
  let field_str = String::from_redis_value(field)?;
  if field_str != expected {
    return Err(RedisError::from((
      redis::ErrorKind::TypeError,
      "Invalid field",
      format!("Expected '{}', found '{}'", expected, field_str),
    )));
  }
  Ok(())
}

pub struct RedisString(String);
impl FromRedisValue for RedisString {
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    match v {
      Value::Data(bytes) => Ok(RedisString(String::from_utf8(bytes.to_vec())?)),
      _ => Err(internal("expecting Value::Data")),
    }
  }
}

impl Display for RedisString {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0.clone())
  }
}

fn bulk_from_redis_value(v: &Value) -> Result<&Vec<Value>, RedisError> {
  match v {
    Value::Bulk(b) => Ok(b),
    _ => Err(internal("expecting Value::Bulk")),
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CollabControlEvent {
  Open {
    workspace_id: String,
    object_id: String,
    collab_type: CollabType,
    doc_state: Vec<u8>,
  },
  Close {
    object_id: String,
  },
}

impl Display for CollabControlEvent {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      CollabControlEvent::Open {
        workspace_id: _,
        object_id,
        collab_type,
        doc_state: _,
      } => f.write_fmt(format_args!(
        "Open collab: object_id:{}|collab_type:{:?}",
        object_id, collab_type,
      )),
      CollabControlEvent::Close { object_id } => {
        f.write_fmt(format_args!("Close collab: object_id:{}", object_id))
      },
    }
  }
}

impl CollabControlEvent {
  pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(self)
  }

  pub fn decode(data: &[u8]) -> Result<Self, serde_json::Error> {
    serde_json::from_slice(data)
  }
}

impl TryFrom<CollabControlEvent> for StreamBinary {
  type Error = StreamError;

  fn try_from(value: CollabControlEvent) -> Result<Self, Self::Error> {
    let raw_data = value.encode()?;
    Ok(StreamBinary(raw_data))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CollabUpdateEvent {
  UpdateV1 { encode_update: Vec<u8> },
}

impl CollabUpdateEvent {
  #[allow(dead_code)]
  fn to_proto(&self) -> proto::collab::CollabUpdateEvent {
    match self {
      CollabUpdateEvent::UpdateV1 { encode_update } => proto::collab::CollabUpdateEvent {
        update: Some(Update::UpdateV1(encode_update.clone())),
      },
    }
  }

  fn from_proto(proto: &proto::collab::CollabUpdateEvent) -> Result<Self, StreamError> {
    match &proto.update {
      None => Err(StreamError::UnexpectedValue(
        "update not set for CollabUpdateEvent proto".to_string(),
      )),
      Some(update) => match update {
        Update::UpdateV1(encode_update) => Ok(CollabUpdateEvent::UpdateV1 {
          encode_update: encode_update.to_vec(),
        }),
      },
    }
  }

  pub fn encode(&self) -> Vec<u8> {
    self.to_proto().encode_to_vec()
  }

  pub fn decode(data: &[u8]) -> Result<Self, StreamError> {
    match prost::Message::decode(data) {
      Ok(proto) => CollabUpdateEvent::from_proto(&proto),
      Err(_) => match bincode::deserialize(data) {
        Ok(event) => Ok(event),
        Err(e) => Err(StreamError::BinCodeSerde(e)),
      },
    }
  }
}

impl TryFrom<CollabUpdateEvent> for StreamBinary {
  type Error = StreamError;

  fn try_from(value: CollabUpdateEvent) -> Result<Self, Self::Error> {
    let raw_data = value.encode();
    Ok(StreamBinary(raw_data))
  }
}

#[cfg(test)]
mod test {

  #[test]
  fn test_collab_update_event_decoding() {
    let encoded_update = vec![1, 2, 3, 4, 5];
    let event = super::CollabUpdateEvent::UpdateV1 {
      encode_update: encoded_update.clone(),
    };
    let encoded = event.encode();
    let decoded = super::CollabUpdateEvent::decode(&encoded).unwrap();
    assert_eq!(event, decoded);
  }
}
