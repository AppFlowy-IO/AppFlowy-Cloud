use std::collections::BTreeMap;
use std::i64;
use std::str::FromStr;

use crate::error::{internal, StreamError};
use redis::{FromRedisValue, RedisError, RedisResult, Value};

#[derive(Debug)]
pub struct CreatedTime {
  pub timestamp_ms: u64,

  // applies if more than one message is sent at the same millisecond
  // this distinguishes the order of the messages
  pub sequence_number: u16,
}

impl CreatedTime {
  fn from_redis_stream_key(bytes: &[u8]) -> Result<Self, StreamError> {
    let s = std::str::from_utf8(bytes)?;
    let parts: Vec<_> = s.splitn(2, '-').collect();

    if parts.len() != 2 {
      return Err(StreamError::InvalidFormat);
    }

    // Directly parse without intermediate assignment.
    let timestamp_ms = u64::from_str(parts[0])?;
    let sequence_number = u16::from_str(parts[1])?;

    Ok(CreatedTime {
      timestamp_ms,
      sequence_number,
    })
  }
}

impl FromRedisValue for CreatedTime {
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    match v {
      Value::Data(stream_key) => CreatedTime::from_redis_stream_key(stream_key).map_err(|_| {
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
pub struct MessageReadByStreamKey(pub BTreeMap<String, Vec<MessageRead>>);

impl FromRedisValue for MessageReadByStreamKey {
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    let mut map: BTreeMap<String, Vec<MessageRead>> = BTreeMap::new();
    if matches!(v, Value::Nil) {
      return Ok(MessageReadByStreamKey(map));
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
        let value = MessageRead::from_redis_value(value)?;
        map.entry(stream_key.clone()).or_default().push(value);
      }
    }

    Ok(MessageReadByStreamKey(map))
  }
}

#[derive(Debug)]
pub struct MessageRead {
  /// user who did the change
  pub uid: i64,
  pub raw_data: Vec<u8>,
  /// only applicable when reading from redis
  pub created_time: CreatedTime,
}

impl FromRedisValue for MessageRead {
  // Optimized parsing function
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    let bulk = bulk_from_redis_value(v)?;
    if bulk.len() != 2 {
      return Err(RedisError::from((
        redis::ErrorKind::TypeError,
        "Invalid length",
        "Expected length of 2 for the outer bulk value".to_string(),
      )));
    }

    let created_time = CreatedTime::from_redis_value(&bulk[0])?;
    let fields = bulk_from_redis_value(&bulk[1])?;
    if fields.len() != 4 {
      return Err(RedisError::from((
        redis::ErrorKind::TypeError,
        "Invalid length",
        "Expected length of 4 for the inner bulk value".to_string(),
      )));
    }

    verify_field(&fields[0], "uid")?;
    let uid = UserId::from_redis_value(&fields[1])?.0;
    verify_field(&fields[2], "data")?;
    let raw_data = Vec::<u8>::from_redis_value(&fields[3])?;

    Ok(MessageRead {
      uid,
      raw_data,
      created_time,
    })
  }

  // Utility function to verify expected field names
}

#[derive(Debug)]
pub struct Message {
  /// user who did the change
  pub uid: i64,
  pub raw_data: Vec<u8>,
}

impl From<MessageRead> for Message {
  fn from(m: MessageRead) -> Self {
    Message {
      uid: m.uid,
      raw_data: m.raw_data,
    }
  }
}

impl Message {
  pub fn into_tuple_array(self) -> [(&'static str, Vec<u8>); 2] {
    static UID: &str = "uid";
    static DATA: &str = "data";
    let mut buf = [0u8; 8];
    buf.copy_from_slice(self.uid.to_be_bytes().as_slice());
    [(UID, buf.to_vec()), (DATA, self.raw_data)]
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
      Value::Data(uid_bytes) => Ok(RedisString(String::from_utf8(uid_bytes.to_vec())?)),
      _ => Err(internal("expecting Value::Data")),
    }
  }
}

impl ToString for RedisString {
  fn to_string(&self) -> String {
    self.0.clone()
  }
}

fn bulk_from_redis_value(v: &Value) -> Result<&Vec<Value>, RedisError> {
  match v {
    Value::Bulk(b) => Ok(b),
    _ => Err(internal("expecting Value::Bulk")),
  }
}

struct UserId(i64);

impl FromRedisValue for UserId {
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    match v {
      Value::Data(uid_bytes) => {
        if uid_bytes.len() == std::mem::size_of::<i64>() {
          let mut buf = [0u8; 8];
          buf.copy_from_slice(uid_bytes);
          let value = i64::from_be_bytes(buf);
          Ok(Self(value))
        } else {
          Err(RedisError::from((
            redis::ErrorKind::TypeError,
            "Invalid UID length",
            format!("Expected 8 bytes, got {}", uid_bytes.len()),
          )))
        }
      },
      _ => Err(RedisError::from((
        redis::ErrorKind::TypeError,
        "Expected Value::Data for UID",
      ))),
    }
  }
}
