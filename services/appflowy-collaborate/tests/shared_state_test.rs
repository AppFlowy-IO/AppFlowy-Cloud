use anyhow::Context;
use appflowy_collaborate::shared_state::RealtimeSharedState;

async fn redis_client() -> redis::Client {
  let redis_uri = "redis://localhost:6379";
  redis::Client::open(redis_uri)
    .context("failed to connect to redis")
    .unwrap()
}

#[tokio::test]
async fn connected_user_test() {
  let redis_client = redis_client().await;
  let shared_state = RealtimeSharedState::new(redis_client.get_connection_manager().await.unwrap());

  let device_id = uuid::Uuid::new_v4().to_string();
  let is_connected = shared_state
    .is_user_connected(&1, &device_id)
    .await
    .unwrap();
  assert!(!is_connected);

  shared_state
    .add_connected_user(1, &device_id)
    .await
    .unwrap();

  let is_connected = shared_state
    .is_user_connected(&1, &device_id)
    .await
    .unwrap();
  assert!(is_connected);

  shared_state
    .remove_connected_user(1, &device_id)
    .await
    .unwrap();

  let is_connected = shared_state
    .is_user_connected(&1, &device_id)
    .await
    .unwrap();
  assert!(!is_connected);
}

#[tokio::test]
async fn remove_all_connected_user_test() {
  let redis_client = redis_client().await;
  let shared_state = RealtimeSharedState::new(redis_client.get_connection_manager().await.unwrap());

  let device_id = uuid::Uuid::new_v4().to_string();
  shared_state
    .add_connected_user(1, &device_id)
    .await
    .unwrap();
  shared_state.remove_all_connected_users().await.unwrap();

  let is_connected = shared_state
    .is_user_connected(&1, &device_id)
    .await
    .unwrap();
  assert!(!is_connected);
}
