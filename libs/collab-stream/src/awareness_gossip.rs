use crate::error::StreamError;
use crate::model::AwarenessStreamUpdate;
use dashmap::DashMap;
use redis::aio::MultiplexedConnection;
use redis::{AsyncCommands, Client, RedisError};
use std::sync::Arc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use uuid::Uuid;

pub struct AwarenessGossip {
  conn: MultiplexedConnection,
  collabs: Arc<DashMap<Uuid, UnboundedSender<AwarenessStreamUpdate>>>,
}

impl AwarenessGossip {
  pub async fn new(client: &Client) -> Result<Self, RedisError> {
    let collabs: Arc<DashMap<Uuid, UnboundedSender<AwarenessStreamUpdate>>> =
      Arc::new(DashMap::new());
    let mut pub_sub = client.get_async_pubsub().await?;
    pub_sub.psubscribe("af:awareness:*").await?;
    let conn = client.get_multiplexed_async_connection().await?;

    let weak_collabs = Arc::downgrade(&collabs);
    let _receive_awareness_pubsub = tokio::spawn(async move {
      let mut stream = pub_sub.into_on_message();
      while let Some(message) = stream.next().await {
        if let Some(collabs) = weak_collabs.upgrade() {
          let collabs_clone = collabs.clone();
          match Self::parse_update(message) {
            Ok((object_id, awareness_update)) => {
              let dropped = if let Some(channel) = collabs_clone.get(&object_id) {
                let channel = channel.value();
                channel.send(awareness_update).is_err()
              } else {
                false
              };
              if dropped {
                collabs.remove(&object_id);
              }
            },
            Err(err) => tracing::error!("failed to parse awareness message: {}", err),
          }
        } else {
          return; // dropped collabs
        }
      }
    });
    Ok(Self { conn, collabs })
  }

  pub async fn send(
    &self,
    workspace_id: &str,
    object_id: &str,
    update: &AwarenessStreamUpdate,
  ) -> Result<(), StreamError> {
    let json = serde_json::to_string(update)?;
    let key = AwarenessGossip::publish_key(workspace_id, object_id);
    let mut pubsub = self.client.get_multiplexed_async_connection().await?;
    pubsub.publish(key, json).await?;
    Ok(())
  }

  pub async fn sink(
    &self,
    workspace_id: &Uuid,
    object_id: &Uuid,
  ) -> Result<AwarenessUpdateSink, StreamError> {
    let sink = AwarenessUpdateSink::new(self.conn.clone(), workspace_id, object_id);
    Ok(sink)
  }

  pub fn awareness_stream(&self, object_id: &Uuid) -> UnboundedReceiver<AwarenessStreamUpdate> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    self.collabs.insert(*object_id, tx);
    rx
  }

  fn parse_update(msg: redis::Msg) -> Result<(Uuid, AwarenessStreamUpdate), StreamError> {
    let channel_name = msg.get_channel_name();
    let last_segment = channel_name
      .split(':')
      .last()
      .ok_or_else(|| StreamError::InvalidStreamKey(channel_name.to_string()))?;
    let object_id =
      Uuid::parse_str(last_segment).map_err(|e| StreamError::InvalidStreamKey(e.to_string()))?;
    let payload = msg.get_payload_bytes();
    let update = serde_json::from_slice::<AwarenessStreamUpdate>(payload)
      .map_err(StreamError::SerdeJsonError)?;
    tracing::trace!("received awareness stream update for {}", channel_name);
    Ok((object_id, update))
  }
}

pub struct AwarenessUpdateSink {
  conn: Mutex<MultiplexedConnection>,
  publish_key: String,
}

impl AwarenessUpdateSink {
  pub fn new(conn: MultiplexedConnection, workspace_id: &Uuid, object_id: &Uuid) -> Self {
    let publish_key = format!("af:awareness:{workspace_id}:{object_id}");
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

#[cfg(test)]
mod test {
  use crate::awareness_gossip::AwarenessGossip;
  use crate::model::AwarenessStreamUpdate;
  use collab::core::awareness::AwarenessUpdate;
  use collab::core::origin::CollabOrigin;
  use uuid::Uuid;

  #[tokio::test]
  async fn subscribe_awareness_change_for_many_collabs() {
    let client = redis::Client::open("redis://127.0.0.1:6379/").unwrap();
    let gossip = AwarenessGossip::new(&client).await.unwrap();
    const COLLAB_COUNT: usize = 10_000;
    let mut collabs = Vec::with_capacity(COLLAB_COUNT);
    for _ in 0..COLLAB_COUNT {
      let workspace_id = Uuid::new_v4();
      let object_id = Uuid::new_v4();
      let sink = gossip.sink(&workspace_id, &object_id).await.unwrap();
      let stream = gossip.awareness_stream(&object_id);
      collabs.push((sink, stream));
    }

    for (sink, _) in collabs.iter() {
      sink
        .send(&AwarenessStreamUpdate {
          data: AwarenessUpdate {
            clients: Default::default(),
          },
          sender: CollabOrigin::Server,
        })
        .await
        .unwrap();
    }

    for (_, stream) in collabs.iter_mut() {
      stream.recv().await.unwrap();
    }
  }
}
