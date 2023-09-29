use redis::{FromRedisValue, RedisError};

#[derive(Debug)]
pub struct CreatedTime {
  pub timestamp_ms: u64,

  // applies if more than one update is sent at the same millisecond
  // this distinguishes the order of the updates
  pub sequence_number: u16,
}

impl CreatedTime {
  fn from_redis_stream_key(bytes: &Vec<u8>) -> Result<Self, &'static str> {
    // Convert the Vec<u8> to a &str
    let s = std::str::from_utf8(bytes).map_err(|_| "Invalid UTF-8 sequence")?;

    // Split the string into two parts by '-'
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 2 {
      return Err("Invalid format");
    }

    // Parse the timestamp and sequence_number parts
    let timestamp_ms = parts[0]
      .parse::<u64>()
      .map_err(|_| "Invalid timestamp format")?;
    let sequence_number = parts[1]
      .parse::<u16>()
      .map_err(|_| "Invalid sequence number format")?;

    Ok(CreatedTime {
      timestamp_ms,
      sequence_number,
    })
  }
}

impl FromRedisValue for CreatedTime {
  fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
    match v {
      redis::Value::Data(stream_key) => {
        CreatedTime::from_redis_stream_key(stream_key).map_err(|_| {
          RedisError::from((
            redis::ErrorKind::TypeError,
            "invalid stream key",
            format!("{:?}", stream_key),
          ))
        })
      },
      x => Err(read_from_redis_error(x)),
    }
  }
}

// representing each document update
#[derive(Default, Debug)]
pub struct CollabUpdate {
  pub uid: String,                       // user who did the change
  pub raw_data: Vec<u8>,                 // YRS data
  pub created_time: Option<CreatedTime>, // only applicable when reading from redis
}

fn read_from_redis_error(unexpected_value: &redis::Value) -> RedisError {
  RedisError::from((
    redis::ErrorKind::TypeError,
    "unexpected value from redis",
    format!("{:?}", unexpected_value),
  ))
}

pub struct RedisString(String);
impl FromRedisValue for RedisString {
  fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
    match v {
      redis::Value::Data(uid_bytes) => Ok(RedisString(
        String::from_utf8(uid_bytes.to_vec()).map_err(|_| read_from_redis_error(v))?,
      )),
      x => {
        println!("----------------------------");
        return Err(read_from_redis_error(x));
      },
    }
  }
}

fn vec_from_redis_value(v: &redis::Value) -> Result<&Vec<redis::Value>, RedisError> {
  match v {
    redis::Value::Bulk(b) => Ok(b),
    x => Err(read_from_redis_error(x)),
  }
}

fn raw_from_redis_value(v: &redis::Value) -> Result<&Vec<u8>, RedisError> {
  match v {
    redis::Value::Data(d) => Ok(d),
    x => Err(read_from_redis_error(x)),
  }
}

impl FromRedisValue for CollabUpdate {
  fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
    let b = vec_from_redis_value(v)?;
    assert_eq!(b.len(), 2);

    let created_time = CreatedTime::from_redis_value(&b[0])?;

    let b = vec_from_redis_value(&b[1])?;
    assert_eq!(b.len(), 4);

    let uid_field = RedisString::from_redis_value(&b[0])?;
    assert_eq!(uid_field.0, "uid");

    let uid = RedisString::from_redis_value(&b[1])?.0;

    let data_field = RedisString::from_redis_value(&b[2])?;
    assert_eq!(data_field.0, "data");

    let yrs_data = raw_from_redis_value(&b[3])?.to_vec();

    Ok(CollabUpdate {
      uid,
      raw_data: yrs_data,
      created_time: Some(created_time),
    })
  }
}

impl<'a> CollabUpdate {
  pub fn to_single_tuple_array(&'a self) -> [(&'static str, &'a [u8]); 2] {
    static UID: &str = "uid";
    static DATA: &str = "data";
    [(UID, &self.uid.as_bytes()), (DATA, &self.raw_data.as_ref())]
  }
}
