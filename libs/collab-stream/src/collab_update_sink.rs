use crate::error::StreamError;
use crate::model::{CollabStreamUpdate, MessageId};
use redis::aio::ConnectionManager;
use redis::cmd;
use tokio::sync::Mutex;

pub struct CollabUpdateSink {
  conn: Mutex<ConnectionManager>,
  stream_key: String,
}

impl CollabUpdateSink {
  pub fn new(conn: ConnectionManager, stream_key: String) -> Self {
    CollabUpdateSink {
      conn: conn.into(),
      stream_key,
    }
  }

  pub async fn send(&self, msg: &CollabStreamUpdate) -> Result<MessageId, StreamError> {
    let mut lock = self.conn.lock().await;
    let msg_id: MessageId = cmd("XADD")
      .arg(&self.stream_key)
      .arg("*")
      .arg("flags")
      .arg(msg.flags)
      .arg("sender")
      .arg(msg.sender.to_string())
      .arg("data")
      .arg(&*msg.data)
      .query_async(&mut *lock)
      .await?;
    Ok(msg_id)
  }
}
