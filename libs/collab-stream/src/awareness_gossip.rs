use crate::error::StreamError;
use crate::model::AwarenessStreamUpdate;
use async_stream::try_stream;
use futures::Stream;
use redis::aio::MultiplexedConnection;
use redis::{AsyncCommands, Client};
use tokio::sync::Mutex;
use tokio_stream::StreamExt;

pub struct AwarenessGossip {
  client: Client,
}

impl AwarenessGossip {
  /// Returns Redis stream key, that's storing entries mapped to/from [AwarenessStreamUpdate].
  pub fn publish_key(workspace_id: &str, object_id: &str) -> String {
    format!("af:awareness:{}:{}", workspace_id, object_id)
  }

  pub fn state_key(workspace_id: &str, object_id: &str) -> String {
    format!("af:awareness_state:{}:{}", workspace_id, object_id)
  }

  pub fn new(client: Client) -> Self {
    Self { client }
  }

  pub async fn sink(
    &self,
    workspace_id: &str,
    object_id: &str,
  ) -> Result<AwarenessUpdateSink, StreamError> {
    tracing::trace!("publishing awareness state for object {}", object_id);
    let mut conn = self.client.get_multiplexed_async_connection().await?;
    // delete existing redis stream from previous versions
    let _: redis::Value = conn
      .del(format!("af:{}:{}:awareness", workspace_id, object_id))
      .await?;
    let sink = AwarenessUpdateSink::new(conn, workspace_id, object_id);
    Ok(sink)
  }

  pub fn awareness_stream(
    &self,
    workspace_id: &str,
    object_id: &str,
  ) -> impl Stream<Item = Result<AwarenessStreamUpdate, StreamError>> + 'static {
    let client = self.client.clone();
    let pubsub_key = Self::publish_key(workspace_id, object_id);
    try_stream! {
        let mut pubsub = client.get_async_pubsub().await?;
        pubsub.psubscribe(pubsub_key.clone()).await?; // try to open subscription
        {
            // from now on, we shouldn't throw any errors here, otherwise punsubscribe won't be called
            let mut stream = pubsub.on_message();
            while let Some(msg) = stream.next().await {
                let update = Self::parse_update(msg)?;
                yield update;
            }
        }
        tracing::trace!("unsubscribing from awareness stream {}", pubsub_key);
        pubsub.punsubscribe(pubsub_key).await?; // close subscription gracefully
    }
  }

  fn parse_update(msg: redis::Msg) -> Result<AwarenessStreamUpdate, StreamError> {
    let channel_name = msg.get_channel_name();
    let payload = msg.get_payload_bytes();
    let update = serde_json::from_slice::<AwarenessStreamUpdate>(payload)
      .map_err(StreamError::SerdeJsonError)?;
    tracing::trace!("received awareness stream update for {}", channel_name);
    Ok(update)
  }
}

pub struct AwarenessUpdateSink {
  conn: Mutex<MultiplexedConnection>,
  publish_key: String,
}

impl AwarenessUpdateSink {
  pub fn new(conn: MultiplexedConnection, workspace_id: &str, object_id: &str) -> Self {
    let publish_key = AwarenessGossip::publish_key(workspace_id, object_id);
    AwarenessUpdateSink {
      conn: conn.into(),
      publish_key,
    }
  }

  pub async fn send(&self, msg: &AwarenessStreamUpdate) -> Result<(), StreamError> {
    let mut conn = self.conn.lock().await;
    Self::notify_awareness_change(&mut conn, &self.publish_key, msg).await?;
    Ok(())
  }

  /// Send a Redis pub-sub message to notify other clients about the awareness change.
  async fn notify_awareness_change(
    conn: &mut MultiplexedConnection,
    pubsub_key: &str,
    update: &AwarenessStreamUpdate,
  ) -> Result<(), StreamError> {
    tracing::trace!("notify awareness change for {}: {:?}", pubsub_key, update);
    let json = serde_json::to_string(update)?;
    let _: redis::Value = conn.publish(pubsub_key, json).await?;
    Ok(())
  }
}
