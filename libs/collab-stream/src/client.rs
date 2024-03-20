use crate::stream::CollabStream;
use redis::aio::ConnectionManager;

pub struct CollabStreamClient {
  connection_manager: ConnectionManager,
}

impl CollabStreamClient {
  pub async fn new(redis_client: redis::Client) -> Result<Self, redis::RedisError> {
    let connection_manager = redis_client.get_connection_manager().await?;
    Ok(Self { connection_manager })
  }

  pub async fn stream(&self, workspace_id: &str, oid: &str) -> CollabStream {
    CollabStream::new(self.connection_manager.clone(), workspace_id, oid)
  }
}
