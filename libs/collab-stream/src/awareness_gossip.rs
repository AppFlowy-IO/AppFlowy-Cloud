use crate::error::StreamError;
use crate::model::AwarenessStreamUpdate;
use async_stream::try_stream;
use bytes::Bytes;
use collab::core::awareness::{AwarenessUpdate, AwarenessUpdateEntry};
use collab::preclude::block::ClientID;
use collab::preclude::updates::encoder::Encode;
use futures::Stream;
use redis::aio::MultiplexedConnection;
use redis::{AsyncCommands, Client};
use std::collections::HashMap;
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
    let conn = self.client.get_multiplexed_async_connection().await?;
    let sink = AwarenessUpdateSink::new(conn, workspace_id, object_id);
    Ok(sink)
  }

  pub async fn get_awareness_state(
    &self,
    workspace_id: &str,
    object_id: &str,
  ) -> Result<Bytes, StreamError> {
    let mut conn = self.client.get_multiplexed_async_connection().await?;
    let key = Self::state_key(workspace_id, object_id);
    let entries: HashMap<ClientID, String> = conn.hgetall(key).await?;
    let mut clients = HashMap::with_capacity(entries.len());
    for (client_id, json) in entries {
      tracing::trace!(
        "got awareness state for client {} with value: {}",
        client_id,
        json
      );
      let entry: AwarenessUpdateEntry = serde_json::from_str(&json)?;
      clients.insert(client_id, entry);
    }
    let update = (AwarenessUpdate { clients }).encode_v1();
    Ok(update.into())
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
  state_key: String,
  publish_key: String,
}

impl AwarenessUpdateSink {
  const EXPIRE_DATE_SECS: i64 = 60 * 60; // 1 hour

  pub fn new(conn: MultiplexedConnection, workspace_id: &str, object_id: &str) -> Self {
    let state_key = AwarenessGossip::state_key(workspace_id, object_id);
    let publish_key = AwarenessGossip::publish_key(workspace_id, object_id);
    AwarenessUpdateSink {
      conn: conn.into(),
      state_key,
      publish_key,
    }
  }

  pub async fn send(&self, msg: &AwarenessStreamUpdate) -> Result<(), StreamError> {
    let mut conn = self.conn.lock().await;
    Self::notify_awareness_change(&mut conn, &self.publish_key, msg).await?;
    Self::merge_awareness_state(&mut conn, &self.state_key, &msg.data).await?;
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

  /// Update redis state entry with the new awareness state.
  ///
  /// NOTE: since updates are stored in Redis map on entry-per-client basis, and they are updated
  /// from web socket messages, we know that the latest incoming update is the most recent one.
  /// For that reason we don't need to use clock-based merge strategy, instead we can just send
  /// the updates as they come.
  async fn merge_awareness_state(
    conn: &mut MultiplexedConnection,
    key: &str,
    update: &AwarenessUpdate,
  ) -> Result<(), StreamError> {
    let mut updated = Vec::with_capacity(update.clients.len());
    for (client_id, entry) in update.clients.iter() {
      if entry.json.is_empty() || &*entry.json == "null" {
        tracing::trace!("removing awareness state for client {}", client_id);
        let _: redis::Value = conn.hdel(key, client_id).await?;
      } else {
        let value = serde_json::to_string(&entry)?;
        tracing::trace!(
          "updating awareness state for client {} with value: {}",
          client_id,
          value
        );
        updated.push((client_id, value));
      }
    }
    if !updated.is_empty() {
      let _: redis::Value = conn.hset_multiple(key, &updated).await?;
      let _: redis::Value = conn.expire(key, Self::EXPIRE_DATE_SECS).await?;
    }
    Ok(())
  }
}
