use crate::client_api_client;
use serde_json::json;

use collab_define::CollabType;

use crate::realtime::test_client::{assert_collab_json, TestClient};
use assert_json_diff::assert_json_eq;
use std::time::Duration;
use storage::collab::FLUSH_PER_UPDATE;

#[tokio::test]
async fn realtime_write_collab_test() {
  let mut client_api = client_api_client();
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let test_client = TestClient::new(&mut client_api, &object_id, collab_type.clone()).await;

  // Edit the collab
  for i in 0..=5 {
    test_client
      .collab
      .lock()
      .insert(&i.to_string(), i.to_string());
  }

  // Wait for the messages to be sent
  tokio::time::sleep(Duration::from_secs(2)).await;
  test_client.disconnect().await;

  assert_collab_json(
    &client_api,
    &object_id,
    &collab_type,
    5,
    json!( {
      "0": "0",
      "1": "1",
      "2": "2",
      "3": "3",
      "4": "4",
      "5": "5",
    }),
  )
  .await;
}

#[tokio::test]
async fn multiple_write_collab_test() {
  let mut client_api = client_api_client();
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let test_client_1 = TestClient::new(&mut client_api, &object_id, collab_type.clone()).await;
  let test_client_2 = TestClient::new(&mut client_api, &object_id, collab_type.clone()).await;

  // Edit the collab
  for i in 0..=FLUSH_PER_UPDATE {
    test_client_1.collab.lock().insert("name", "AppFlowy");
    tokio::time::sleep(Duration::from_millis(10)).await;
  }

  assert_collab_json(
    &client_api,
    &object_id,
    &collab_type,
    5,
    json!({
      "name": "AppFlowy"
    }),
  )
  .await;

  // tokio::time::sleep(Duration::from_secs(6)).await;
  //
  // let json_1 = test_client_1.collab.lock().to_json_value();
  // let json_2 = test_client_2.collab.lock().to_json_value();
  // assert_json_eq!(json_1, json_2);
}
