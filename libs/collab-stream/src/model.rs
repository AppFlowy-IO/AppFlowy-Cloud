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
pub struct Message {
  /// user who did the change
  pub uid: i64,
  pub raw_data: Vec<u8>,
}

#[derive(Debug)]
pub struct MessageReadById(pub BTreeMap<String, Vec<MessageRead>>);

impl FromRedisValue for MessageReadById {
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    let mut map: BTreeMap<String, Vec<MessageRead>> = BTreeMap::new();

    let updates_by_ids = bulk_from_redis_value(v)?.iter();
    for updates in updates_by_ids {
      let key_values = bulk_from_redis_value(updates)?;
      let key = RedisString::from_redis_value(&key_values[0])?.0;
      let values = bulk_from_redis_value(&key_values[1])?.iter();
      for value in values {
        let value = MessageRead::from_redis_value(value)?;
        map.entry(key.clone()).or_insert_with(Vec::new).push(value);
      }
    }

    Ok(MessageReadById(map))
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

fn data_from_redis_value(v: &Value) -> Result<&Vec<u8>, RedisError> {
  match v {
    Value::Data(d) => Ok(d),
    _ => Err(internal("expecting Value::Data")),
  }
}

impl FromRedisValue for MessageRead {
  // Optimized parsing function
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    let b = bulk_from_redis_value(v)?;
    if b.len() != 2 {
      return Err(RedisError::from((
        redis::ErrorKind::TypeError,
        "Invalid length",
        "Expected length of 2 for the outer bulk value".to_string(),
      )));
    }

    let created_time = CreatedTime::from_redis_value(&b[0])?;
    let fields = bulk_from_redis_value(&b[1])?;
    if fields.len() != 4 {
      return Err(RedisError::from((
        redis::ErrorKind::TypeError,
        "Invalid length",
        "Expected length of 4 for the inner bulk value".to_string(),
      )));
    }

    // Simplified field verification and value extraction
    verify_field(&fields[0], "uid")?;
    let uid = i64::from_redis_value(&fields[1])?;

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

impl Message {
  pub fn into_tuple_array(self) -> [(&'static str, Vec<u8>); 2] {
    static UID: &str = "uid";
    static DATA: &str = "data";
    [
      (UID, self.uid.to_be_bytes().to_vec()),
      (DATA, self.raw_data),
    ]
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
