use crate::collab_update_sink::{AwarenessUpdateSink, CollabUpdateSink};
use crate::error::{internal, StreamError};
use crate::lease::{Lease, LeaseAcquisition};
use crate::model::{AwarenessStreamUpdate, CollabStreamUpdate, MessageId};
use crate::stream_group::{StreamConfig, StreamGroup};
use crate::stream_router::{StreamRouter, StreamRouterOptions};
use futures::Stream;
use redis::aio::ConnectionManager;
use redis::streams::StreamReadOptions;
use redis::{AsyncCommands, FromRedisValue};
use std::sync::Arc;
use std::time::Duration;
use tracing::error;

pub const CONTROL_STREAM_KEY: &str = "af_collab_control";

#[derive(Clone)]
pub struct CollabRedisStream {
  connection_manager: ConnectionManager,
  stream_router: Arc<StreamRouter>,
}

impl CollabRedisStream {
  pub const LEASE_TTL: Duration = Duration::from_secs(60);

  pub async fn new(redis_client: redis::Client) -> Result<Self, redis::RedisError> {
    let router_options = StreamRouterOptions {
      worker_count: 10,
      xread_streams: 100,
      xread_block_millis: Some(100),
      xread_count: None,
    };
    let stream_router = Arc::new(StreamRouter::with_options(&redis_client, router_options)?);
    let connection_manager = redis_client.get_connection_manager().await?;
    Ok(Self::new_with_connection_manager(
      connection_manager,
      stream_router,
    ))
  }

  pub fn new_with_connection_manager(
    connection_manager: ConnectionManager,
    stream_router: Arc<StreamRouter>,
  ) -> Self {
    Self {
      connection_manager,
      stream_router,
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

  pub async fn collab_control_stream(
    &self,
    key: &str,
    group_name: &str,
  ) -> Result<StreamGroup, StreamError> {
    let mut group = StreamGroup::new_with_config(
      key.to_string(),
      group_name,
      self.connection_manager.clone(),
      StreamConfig::new().with_max_len(1000),
    );

    // don't return error when create consumer group failed
    if let Err(err) = group.ensure_consumer_group().await {
      error!("Failed to ensure consumer group: {}", err);
    }

    Ok(group)
  }

  pub async fn collab_update_stream_group(
    &self,
    workspace_id: &str,
    oid: &str,
    group_name: &str,
  ) -> Result<StreamGroup, StreamError> {
    let stream_key = format!("af_collab_update-{}-{}", workspace_id, oid);
    let mut group = StreamGroup::new_with_config(
      stream_key,
      group_name,
      self.connection_manager.clone(),
      StreamConfig::new()
        // 2000 messages
        .with_max_len(2000)
        // 12 hours
        .with_expire_time(60 * 60 * 12),
    );
    group.ensure_consumer_group().await?;
    Ok(group)
  }

  pub fn collab_update_sink(&self, workspace_id: &str, object_id: &str) -> CollabUpdateSink {
    let stream_key = CollabStreamUpdate::stream_key(workspace_id, object_id);
    CollabUpdateSink::new(self.connection_manager.clone(), stream_key)
  }

  pub fn awareness_update_sink(&self, workspace_id: &str, object_id: &str) -> AwarenessUpdateSink {
    let stream_key = AwarenessStreamUpdate::stream_key(workspace_id, object_id);
    AwarenessUpdateSink::new(self.connection_manager.clone(), stream_key)
  }

  pub fn collab_updates(
    &self,
    workspace_id: &str,
    object_id: &str,
    since: Option<MessageId>,
    keep_alive: bool,
  ) -> impl Stream<Item = Result<(MessageId, CollabStreamUpdate), StreamError>> {
    let stream_key = CollabStreamUpdate::stream_key(workspace_id, object_id);
    let since = since.map(|id| id.to_string());
    let mut reader = self.stream_router.observe(stream_key, since);
    async_stream::try_stream! {
      while let Some((message_id, fields)) = reader.recv().await {
        let message_id = MessageId::try_from(message_id).map_err(|e| internal(e.to_string()))?;
        let collab_update = CollabStreamUpdate::try_from(fields)?;
        yield (message_id, collab_update);
      }
    }
  }

  pub fn awareness_updates(
    &self,
    workspace_id: &str,
    object_id: &str,
    since: Option<MessageId>,
  ) -> impl Stream<Item = Result<AwarenessStreamUpdate, StreamError>> {
    let stream_key = AwarenessStreamUpdate::stream_key(workspace_id, object_id);
    let since = since.map(|id| id.to_string());
    let mut reader = self.stream_router.observe(stream_key, since);
    async_stream::try_stream! {
      while let Some((_message_id, fields)) = reader.recv().await {
        let awareness_update = AwarenessStreamUpdate::try_from(fields)?;
        yield awareness_update;
      }
    }
  }

  pub async fn prune_stream(
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
      "pruned redis stream `{}` <= `{}` ({} objects)",
      stream_key,
      message_id,
      count
    );
    Ok(count)
  }
}
