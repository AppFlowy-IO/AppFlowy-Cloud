use crate::error::StreamError;
use crate::pubsub::{CollabStreamPub, CollabStreamSub};
use crate::stream::CollabStream;
use crate::stream_group::StreamGroup;
use redis::aio::ConnectionManager;

#[derive(Clone)]
pub struct CollabRedisStream {
  connection_manager: ConnectionManager,
}

impl CollabRedisStream {
  pub async fn new(redis_client: redis::Client) -> Result<Self, redis::RedisError> {
    let connection_manager = redis_client.get_connection_manager().await?;
    Ok(Self::new_with_connection_manager(connection_manager).await)
  }

  pub async fn new_with_connection_manager(connection_manager: ConnectionManager) -> Self {
    Self { connection_manager }
  }

  pub async fn stream(&self, workspace_id: &str, oid: &str) -> CollabStream {
    CollabStream::new(workspace_id, oid, self.connection_manager.clone())
  }

  pub async fn collab_control_stream(&self, group_name: &str) -> Result<StreamGroup, StreamError> {
    let stream_key = "af_collab_control".to_string();
    let mut group = StreamGroup::new(stream_key, group_name, self.connection_manager.clone());
    group.ensure_consumer_group().await?;
    Ok(group)
  }

  pub async fn collab_update_stream(
    &self,
    workspace_id: &str,
    oid: &str,
    group_name: &str,
  ) -> Result<StreamGroup, StreamError> {
    let stream_key = format!("af_collab_update-{}-{}", workspace_id, oid);
    let mut group = StreamGroup::new(stream_key, group_name, self.connection_manager.clone());
    group.ensure_consumer_group().await?;
    Ok(group)
  }
}

pub struct PubSubClient {
  redis_client: redis::Client,
  connection_manager: ConnectionManager,
}

impl PubSubClient {
  pub async fn new(redis_client: redis::Client) -> Result<Self, redis::RedisError> {
    let connection_manager = redis_client.get_connection_manager().await?;
    Ok(Self {
      redis_client,
      connection_manager,
    })
  }

  pub async fn collab_pub(&self) -> CollabStreamPub {
    CollabStreamPub::new(self.connection_manager.clone())
  }

  #[allow(deprecated)]
  pub async fn collab_sub(&self) -> Result<CollabStreamSub, StreamError> {
    let conn = self.redis_client.get_async_connection().await?;
    Ok(CollabStreamSub::new(conn))
  }
}
