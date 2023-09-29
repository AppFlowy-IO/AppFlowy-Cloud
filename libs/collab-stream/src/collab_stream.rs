use crate::collab_update::{CollabUpdate, CreatedTime};
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use tokio_stream::Stream;

pub struct CollabStream {
  connection_manager: ConnectionManager,
  redis_stream_key: String,
}

impl CollabStream {
  pub fn new(connection_manager: ConnectionManager, oid: &str, partition_key: i64) -> Self {
    let redis_stream_key = stream_id_from_af_collab(oid, partition_key);
    Self {
      connection_manager,
      redis_stream_key,
    }
  }

  pub async fn read_all_updates(&mut self) -> Result<Vec<CollabUpdate>, redis::RedisError> {
    self
      .connection_manager
      .xrange_all(&self.redis_stream_key)
      .await
  }

  pub async fn insert_one_update(
    &mut self,
    collab_update: CollabUpdate,
  ) -> Result<CreatedTime, redis::RedisError> {
    self
      .connection_manager
      .xadd(
        &self.redis_stream_key,
        "*",
        &collab_update.to_single_tuple_array(),
      )
      .await
  }

  pub async fn listen_for_updates(&self) -> Box<dyn Stream<Item = CollabUpdate>> {
    todo!()
  }
}

fn stream_id_from_af_collab(oid: &str, partition_key: i64) -> String {
  format!("af_collab-{}-{}", oid, partition_key)
}
