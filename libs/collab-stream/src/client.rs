use crate::awareness_gossip::{AwarenessGossip, AwarenessUpdateSink};
use crate::collab_update_sink::CollabUpdateSink;
use crate::error::StreamError;
use crate::lease::{Lease, LeaseAcquisition};
use crate::metrics::CollabStreamMetrics;
use crate::model::{AwarenessStreamUpdate, CollabStreamUpdate, MessageId, UpdateStreamMessage};
use crate::stream_router::{FromRedisStream, StreamRouter, StreamRouterOptions};
use collab_entity::CollabType;
use futures::{Stream, StreamExt};
use redis::aio::ConnectionManager;
use redis::streams::StreamReadReply;
use redis::{AsyncCommands, FromRedisValue};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use uuid::Uuid;

#[derive(Clone)]
pub struct CollabRedisStream {
  connection_manager: ConnectionManager,
  stream_router: Arc<StreamRouter>,
  awareness_gossip: Arc<AwarenessGossip>,
}

impl CollabRedisStream {
  pub const LEASE_TTL: Duration = Duration::from_secs(60);

  pub async fn new(
    redis_client: redis::Client,
    metrics: Arc<CollabStreamMetrics>,
  ) -> Result<Self, redis::RedisError> {
    let router_options = StreamRouterOptions {
      worker_count: 60,
      xread_streams: 100,
      xread_block_millis: Some(5000),
      xread_count: None,
    };
    let stream_router = Arc::new(StreamRouter::with_options(
      &redis_client,
      metrics,
      router_options,
    )?);
    let awareness_gossip = Arc::new(AwarenessGossip::new(&redis_client).await?);
    let connection_manager = redis_client.get_connection_manager().await?;
    Ok(Self::new_with_connection_manager(
      connection_manager,
      stream_router,
      awareness_gossip,
    ))
  }

  pub fn new_with_connection_manager(
    connection_manager: ConnectionManager,
    stream_router: Arc<StreamRouter>,
    awareness_gossip: Arc<AwarenessGossip>,
  ) -> Self {
    Self {
      connection_manager,
      stream_router,
      awareness_gossip,
    }
  }

  pub async fn lease(
    &self,
    workspace_id: &str,
    object_id: &str,
  ) -> Result<Option<LeaseAcquisition>, StreamError> {
    let lease_key = format!("af:{}:{}:snapshot_lease", workspace_id, object_id);
    self
      .connection_manager
      .lease(lease_key, Self::LEASE_TTL)
      .await
  }

  pub fn collab_update_sink(
    &self,
    workspace_id: &Uuid,
    object_id: &Uuid,
    collab_type: CollabType,
  ) -> CollabUpdateSink {
    let stream_key = UpdateStreamMessage::stream_key(workspace_id);
    CollabUpdateSink::new(
      self.connection_manager.clone(),
      stream_key,
      *object_id,
      collab_type,
    )
  }

  pub async fn awareness_update_sink(
    &self,
    workspace_id: &Uuid,
    object_id: &Uuid,
  ) -> Result<AwarenessUpdateSink, StreamError> {
    self.awareness_gossip.sink(workspace_id, object_id).await
  }

  /// Reads all collab updates for a given `workspace_id`:`object_id` entry, starting
  /// from a given message id. Once Redis stream return no more results, the stream will be closed.
  pub async fn current_collab_updates(
    &self,
    workspace_id: &Uuid,
    object_id: &Uuid,
    since: Option<MessageId>,
  ) -> Result<Vec<(MessageId, CollabStreamUpdate)>, StreamError> {
    let stream_key = UpdateStreamMessage::stream_key(workspace_id);
    let since = since.unwrap_or_default().to_string();
    let mut conn = self.connection_manager.clone();
    let mut result = Vec::new();
    let mut reply: StreamReadReply = conn.xread(&[&stream_key], &[&since]).await?;
    let oid = object_id.to_string();
    if let Some(key) = reply.keys.pop() {
      if key.key == stream_key {
        for stream_id in key.ids {
          let msg_oid = stream_id
            .map
            .get("oid")
            .and_then(|v| String::from_redis_value(v).ok())
            .unwrap_or_default();
          if msg_oid != oid {
            continue; // this is not the object we are looking for
          }
          let message = UpdateStreamMessage::from_redis_stream(&stream_id.id, &stream_id.map)
            .map_err(StreamError::Internal)?;
          let message_id = MessageId::from(message.last_message_id);
          let stream_update = CollabStreamUpdate::from(message);
          tracing::trace!(
            "replaying current collab update `{}` for {}",
            message_id,
            msg_oid
          );
          result.push((message_id, stream_update));
        }
      }
    }
    Ok(result)
  }

  /// Reads all collab updates for a given `workspace_id`:`object_id` entry, starting
  /// from a given message id. This stream will be kept alive and pass over all future messages
  /// coming from corresponding Redis stream until explicitly closed.
  pub fn live_collab_updates(
    &self,
    workspace_id: &Uuid,
    object_id: &Uuid,
    since: Option<MessageId>,
  ) -> impl Stream<Item = Result<(MessageId, CollabStreamUpdate), StreamError>> {
    let stream_key = UpdateStreamMessage::stream_key(workspace_id);
    let since = since.map(|id| id.to_string());
    let object_id = *object_id;
    let mut reader = self
      .stream_router
      .observe::<UpdateStreamMessage>(stream_key, since);
    async_stream::try_stream! {
      while let Some(Ok(msg)) = reader.next().await {
        if msg.object_id == object_id {
          let message_id = MessageId::from(msg.last_message_id);
          let collab_update = CollabStreamUpdate::from(msg);
          tracing::trace!("incoming collab update `{}`", message_id);
          yield (message_id, collab_update);
        }
      }
    }
  }

  pub fn awareness_updates(&self, object_id: &Uuid) -> UnboundedReceiver<AwarenessStreamUpdate> {
    self.awareness_gossip.collab_awareness_stream(object_id)
  }

  pub async fn delete_stream_messages(
    &self,
    stream_key: &str,
    message_ids: &[MessageId],
  ) -> Result<(), StreamError> {
    let mut conn = self.connection_manager.clone();
    let _: redis::Value = conn.xdel(stream_key, message_ids).await?;
    Ok(())
  }

  pub async fn prune_update_stream(
    &self,
    stream_key: &str,
    mut message_id: MessageId,
  ) -> Result<usize, StreamError> {
    let mut conn = self.connection_manager.clone();
    // we want to delete everything <= message_id
    message_id.sequence_number += 1;
    let value = conn
      .send_packed_command(
        redis::cmd("XTRIM")
          .arg(stream_key)
          .arg("MINID")
          .arg(format!("{}", message_id)),
      )
      .await?;
    let count = usize::from_redis_value(&value)?;
    drop(conn);
    tracing::debug!(
      "pruned redis update stream `{}` <= `{}` ({} objects)",
      stream_key,
      message_id,
      count
    );
    Ok(count)
  }

  pub async fn prune_awareness_stream(&self, stream_key: &str) -> Result<(), StreamError> {
    let mut conn = self.connection_manager.clone();
    let value = conn
      .send_packed_command(
        redis::cmd("XTRIM")
          .arg(stream_key)
          .arg("MAXLEN")
          .arg("=")
          .arg(0),
      )
      .await?;
    let count = usize::from_redis_value(&value)?;
    drop(conn);
    tracing::debug!(
      "pruned redis awareness stream {} ({} objects)",
      stream_key,
      count
    );
    Ok(())
  }
}
