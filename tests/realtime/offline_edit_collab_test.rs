use crate::client::utils::generate_unique_registered_user;
use crate::realtime::test_client::{assert_collab_json, TestClient};
use assert_json_diff::assert_json_eq;
use collab_define::CollabType;
use serde_json::json;
use shared_entity::error_code::ErrorCode;
use sqlx::types::uuid;
use std::time::Duration;

use storage_entity::QueryCollabParams;

#[tokio::test]
async fn ws_reconnect_sync_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let mut test_client = TestClient::new().await;
  test_client.create(&object_id, collab_type.clone()).await;

  // Disconnect the client and edit the collab. The updates will not be sent to the server.
  test_client.disconnect().await;
  for i in 0..=5 {
    test_client
      .collab_by_object_id
      .get_mut(&object_id)
      .unwrap()
      .collab
      .lock()
      .insert(&i.to_string(), i.to_string());
  }

  // it will return RecordNotFound error when trying to get the collab from the server
  let err = test_client
    .api_client
    .get_collab(QueryCollabParams {
      object_id: object_id.clone(),
      collab_type: collab_type.clone(),
    })
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::RecordNotFound);

  // After reconnect the collab should be synced to the server.
  test_client.reconnect().await;
  // Wait for the messages to be sent
  test_client.wait_object_sync_complete(&object_id).await;

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
async fn edit_document_with_one_client_online_and_other_offline_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let registered_user = generate_unique_registered_user().await;

  let mut client_1 = TestClient::new_with_user(registered_user.clone()).await;
  client_1.create(&object_id, collab_type.clone()).await;
  client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "work");

  client_1.wait_object_sync_complete(&object_id).await;

  let mut client_2 = TestClient::new_with_user(registered_user.clone()).await;
  client_2.create(&object_id, collab_type.clone()).await;
  client_2.wait_ws_connected().await;
  client_2.disconnect().await;

  client_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "workspace");

  client_2.reconnect().await;
  client_2.wait_object_sync_complete(&object_id).await;

  // Wait 2 secs that let the server broadcast the update to the client_1.
  // It might fail if the server is slow.
  tokio::time::sleep(Duration::from_secs(2)).await;

  let expected_json = json!({
    "name": "workspace"
  });
  let json_1 = client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .to_json_value();
  let json_2 = client_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .to_json_value();

  // println!("json_1: {}", json_1);
  // println!("json_2: {}", json_2);
  assert_json_eq!(json_1, expected_json);
  assert_json_eq!(json_2, expected_json);
}
