use anyhow::Context;
use collab_stream::client::CollabStreamClient;

pub async fn redis_client() -> redis::Client {
  let redis_uri = "redis://localhost:6379";
  redis::Client::open(redis_uri)
    .context("failed to connect to redis")
    .unwrap()
}

pub async fn stream_client() -> CollabStreamClient {
  let redis_client = redis_client().await;
  CollabStreamClient::new(redis_client)
    .await
    .context("failed to create stream client")
    .unwrap()
}
