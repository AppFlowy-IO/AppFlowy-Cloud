use crate::error::StreamError;
use futures::stream::BoxStream;
use futures::StreamExt;
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
    self.conn.publish(ACTIVE_COLLAB_CHANNEL, message).await?;
    Ok(())
  }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PubSubMessage {
  pub workspace_id: String,
  pub oid: String,
}

impl PubSubMessage {
  pub fn from_vec(vec: &[u8]) -> Result<Self, StreamError> {
    let message = bincode::deserialize(vec)?;
    Ok(message)
  }
}

impl ToRedisArgs for PubSubMessage {
  fn write_redis_args<W>(&self, out: &mut W)
  where
    W: ?Sized + RedisWrite,
  {
    let json = bincode::serialize(self).unwrap();
    json.write_redis_args(out);
  }
}
