use crate::error::StreamError;
use crate::model::{CollabStreamUpdate, MessageId, UpdateStreamMessage};
use appflowy_proto::ObjectId;
use collab_entity::CollabType;
use redis::aio::ConnectionManager;
use tokio::sync::Mutex;

pub struct CollabUpdateSink {
  conn: Mutex<ConnectionManager>,
  stream_key: String,
  object_id: ObjectId,
  collab_type: CollabType,
}

impl CollabUpdateSink {
  pub fn new(
    conn: ConnectionManager,
    stream_key: String,
    object_id: ObjectId,
    collab_type: CollabType,
  ) -> Self {
    CollabUpdateSink {
      conn: conn.into(),
      stream_key,
      object_id,
      collab_type,
    }
  }

  pub async fn send(&self, msg: &CollabStreamUpdate) -> Result<MessageId, StreamError> {
    let mut lock = self.conn.lock().await;
    let msg_id: String = UpdateStreamMessage::prepare_command(
      &self.stream_key,
      &self.object_id,
      self.collab_type,
      &msg.sender,
      msg.data.clone(),
    )
    .query_async(&mut *lock)
    .await?;
    MessageId::try_from(&*msg_id)
  }
}
