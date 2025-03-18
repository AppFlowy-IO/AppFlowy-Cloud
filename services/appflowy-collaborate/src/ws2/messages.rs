use anyhow::anyhow;
use appflowy_proto::{ObjectId, Rid, UpdateFlags};
use bytes::Bytes;
use collab_stream::stream_router::{FromRedisStream, RedisMap};
use redis::FromRedisValue;
use std::fmt::Display;
use std::str::FromStr;
use yrs::block::ClientID;

#[derive(Debug, PartialEq)]
pub struct UpdateStreamMessage {
  pub last_message_id: Rid,
  pub sender: ClientID,
  pub object_id: ObjectId,
  pub update_flags: UpdateFlags,
  pub update: Bytes,
}

impl FromRedisStream for UpdateStreamMessage {
  type Error = anyhow::Error;
  fn from_redis_stream(msg_id: String, fields: RedisMap) -> Result<Self, Self::Error> {
    let last_message_id = Rid::from_str(&msg_id).map_err(|err| anyhow!("{}", err))?;
    let object_id = fields
      .get("oid")
      .ok_or_else(|| anyhow!("expecting field `object_id`"))?;
    let object_id = ObjectId::from_redis_value(object_id).map_err(|err| anyhow!("{}", err))?;
    let sender = fields
      .get("sender")
      .ok_or_else(|| anyhow!("expecting field `sender`"))?;
    let sender = ClientID::from_redis_value(sender)?;
    let update_flags = match fields.get("flags") {
      None => UpdateFlags::default(),
      Some(flags) => u8::from_redis_value(flags).unwrap_or(0).try_into()?,
    };
    let update = fields
      .get("data")
      .ok_or_else(|| anyhow!("expecting field `data`"))?;
    let update: Bytes = FromRedisValue::from_redis_value(update)?;
    Ok(UpdateStreamMessage {
      last_message_id,
      sender,
      object_id,
      update_flags,
      update,
    })
  }
}
