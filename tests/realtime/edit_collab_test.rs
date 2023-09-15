use crate::client_api_client;
use serde_json::json;

use collab_define::CollabType;

use crate::realtime::test_client::{assert_collab_json, TestClient};

use assert_json_diff::assert_json_eq;
use std::time::Duration;
use storage::collab::FLUSH_PER_UPDATE;

#[tokio::test]
async fn realtime_write_collab_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let mut test_client = TestClient::new(&object_id, collab_type.clone()).await;

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
    &mut test_client.api_client,
    &object_id,
    &collab_type,
    3,
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
async fn one_direction_peer_sync_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new(&object_id, collab_type.clone()).await;
  let client_2 = TestClient::new(&object_id, collab_type.clone()).await;

  // Edit the collab from client 1 and then the server will broadcast to client 2
  for _i in 0..=FLUSH_PER_UPDATE {
    client_1.collab.lock().insert("name", "AppFlowy");
    tokio::time::sleep(Duration::from_millis(10)).await;
  }

  assert_collab_json(
    &mut client_1.api_client,
    &object_id,
    &collab_type,
    5,
    json!({
      "name": "AppFlowy"
    }),
  )
  .await;

  let json_1 = client_1.collab.lock().to_json_value();
  let json_2 = client_2.collab.lock().to_json_value();
  assert_json_eq!(json_1, json_2);
}

#[tokio::test]
async fn two_direction_peer_sync_test() {
  let _client_api = client_api_client();
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let client_1 = TestClient::new(&object_id, collab_type.clone()).await;
  let client_2 = TestClient::new(&object_id, collab_type.clone()).await;

  client_1.collab.lock().insert("name", "AppFlowy");
  tokio::time::sleep(Duration::from_millis(10)).await;

  client_2
    .collab
    .lock()
    .insert("support platform", "macOS, Windows, Linux, iOS, Android");
  tokio::time::sleep(Duration::from_millis(1000)).await;

  let json_1 = client_1.collab.lock().to_json_value();
  let json_2 = client_2.collab.lock().to_json_value();
  assert_json_eq!(
    json_1,
    json!({
      "name": "AppFlowy",
      "support platform": "macOS, Windows, Linux, iOS, Android"
    })
  );
  assert_json_eq!(json_1, json_2);
}

#[tokio::test]
async fn client_init_sync_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let client_1 = TestClient::new(&object_id, collab_type.clone()).await;
  client_1.collab.lock().insert("name", "AppFlowy");
  tokio::time::sleep(Duration::from_millis(10)).await;

  // Open the collab from client 2. After the initial sync, the server will send the missing updates to client_2.
  let client_2 = TestClient::new(&object_id, collab_type.clone()).await;
  tokio::time::sleep(Duration::from_millis(1000)).await;
  let json_1 = client_1.collab.lock().to_json_value();
  let json_2 = client_2.collab.lock().to_json_value();
  assert_json_eq!(
    json_1,
    json!({
      "name": "AppFlowy",
    })
  );
  assert_json_eq!(json_1, json_2);
}

#[tokio::test]
async fn multiple_collab_edit_test() {
  let collab_type = CollabType::Document;

  let object_id_1 = uuid::Uuid::new_v4().to_string();
  let mut client_1 = TestClient::new(&object_id_1, collab_type.clone()).await;

  let object_id_2 = uuid::Uuid::new_v4().to_string();
  let mut client_2 = TestClient::new(&object_id_2, collab_type.clone()).await;

  let object_id_3 = uuid::Uuid::new_v4().to_string();
  let mut client_3 = TestClient::new(&object_id_3, collab_type.clone()).await;

  client_1.collab.lock().insert("title", "I am client 1");
  client_2.collab.lock().insert("title", "I am client 2");
  client_3.collab.lock().insert("title", "I am client 3");
  tokio::time::sleep(Duration::from_secs(2)).await;

  assert_collab_json(
    &mut client_1.api_client,
    &object_id_1,
    &collab_type,
    3,
    json!( {
      "title": "I am client 1"
    }),
  )
  .await;

  assert_collab_json(
    &mut client_2.api_client,
    &object_id_2,
    &collab_type,
    3,
    json!( {
      "title": "I am client 2"
    }),
  )
  .await;
  assert_collab_json(
    &mut client_3.api_client,
    &object_id_3,
    &collab_type,
    3,
    json!( {
      "title": "I am client 3"
    }),
  )
  .await;
}
