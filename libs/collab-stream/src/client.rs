use redis::aio::ConnectionManager;

use crate::collab_stream::CollabStream;

pub struct CollabStreamClient {
  connection_manager: ConnectionManager,
}

impl CollabStreamClient {
  pub async fn new(redis_client: redis::Client) -> Result<Self, redis::RedisError> {
    let connection_manager = redis_client.get_tokio_connection_manager().await?;
    Ok(Self { connection_manager })
  }

  pub async fn stream(&self, oid: &str, partition_key: i64) -> CollabStream {
    CollabStream::new(self.connection_manager.clone(), oid, partition_key)
  }
}
