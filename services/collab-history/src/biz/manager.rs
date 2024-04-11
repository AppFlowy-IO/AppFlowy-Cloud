use crate::biz::open_handle::OpenCollabHandle;
use collab_stream::client::CollabRedisStream;
use dashmap::DashMap;

pub struct OpenCollabManager {
  handles: DashMap<String, OpenCollabHandle>,
  redis_stream: CollabRedisStream,
}

impl OpenCollabManager {
  pub async fn new(redis_stream: CollabRedisStream) -> Self {
    Self {
      handles: Default::default(),
      redis_stream,
    }
  }
}
