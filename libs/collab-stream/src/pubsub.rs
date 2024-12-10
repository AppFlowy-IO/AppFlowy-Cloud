use crate::error::StreamError;
use collab_entity::proto;
use futures::stream::BoxStream;
use futures::StreamExt;
use prost::Message;
#[allow(deprecated)]
use redis::aio::{Connection, ConnectionManager};
use redis::{AsyncCommands, RedisWrite, ToRedisArgs};
use serde::{Deserialize, Serialize};
use tracing::instrument;

const ACTIVE_COLLAB_CHANNEL: &str = "active_collab_channel";

pub struct CollabStreamSub {
  #[allow(deprecated)]
  conn: Connection,
}

impl CollabStreamSub {
  #[allow(deprecated)]
  pub fn new(conn: Connection) -> Self {
    Self { conn }
  }

  pub async fn subscribe(
    self,
  ) -> Result<BoxStream<'static, Result<PubSubMessage, StreamError>>, StreamError> {
    let mut pubsub = self.conn.into_pubsub();
    pubsub.subscribe(ACTIVE_COLLAB_CHANNEL).await?;

    let message_stream = pubsub
      .into_on_message()
      .then(|msg| async move {
        let payload = msg.get_payload_bytes();
        PubSubMessage::from_vec(payload)
      })
      .boxed();
    Ok(message_stream)
  }
}

pub struct CollabStreamPub {
  conn: ConnectionManager,
}

impl CollabStreamPub {
  pub fn new(conn: ConnectionManager) -> Self {
    Self { conn }
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn publish(&mut self, message: PubSubMessage) -> Result<(), StreamError> {
    let () = self.conn.publish(ACTIVE_COLLAB_CHANNEL, message).await?;
    Ok(())
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PubSubMessage {
  pub workspace_id: String,
  pub oid: String,
}

impl PubSubMessage {
  #[allow(dead_code)]
  fn to_proto(&self) -> proto::collab::ActiveCollabId {
    proto::collab::ActiveCollabId {
      workspace_id: self.workspace_id.clone(),
      oid: self.oid.clone(),
    }
  }

  fn from_proto(proto: &proto::collab::ActiveCollabId) -> Self {
    Self {
      workspace_id: proto.workspace_id.clone(),
      oid: proto.oid.clone(),
    }
  }

  pub fn from_vec(vec: &[u8]) -> Result<Self, StreamError> {
    match Message::decode(vec) {
      Ok(proto) => Ok(Self::from_proto(&proto)),
      Err(_) => match bincode::deserialize(vec) {
        Ok(event) => Ok(event),
        Err(e) => Err(StreamError::BinCodeSerde(e)),
      },
    }
  }
}

impl ToRedisArgs for PubSubMessage {
  fn write_redis_args<W>(&self, out: &mut W)
  where
    W: ?Sized + RedisWrite,
  {
    let proto = self.to_proto().encode_to_vec();
    proto.write_redis_args(out);
  }
}

#[cfg(test)]
mod test {
  use prost::Message;

  #[test]
  fn test_pubsub_message_decoding() {
    let message = super::PubSubMessage {
      workspace_id: "1".to_string(),
      oid: "o1".to_string(),
    };
    let bincode_encoded = bincode::serialize(&message).unwrap();
    let protobuf_encoded = message.to_proto().encode_to_vec();
    let decoded_from_bincode = super::PubSubMessage::from_vec(&bincode_encoded).unwrap();
    let decoded_from_protobuf = super::PubSubMessage::from_vec(&protobuf_encoded).unwrap();
    assert_eq!(message, decoded_from_bincode);
    assert_eq!(message, decoded_from_protobuf);
  }
}
