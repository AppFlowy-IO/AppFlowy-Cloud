use std::time::Duration;

use client_api::entity::AFRole;
use client_api_test::{assert_client_collab_include_value, TestClient};
use collab_entity::CollabType;
use serde_json::{json, Value};
use tokio::time::sleep;
use uuid::Uuid;

#[tokio::test]
async fn client_apply_update_find_missing_update_test() {
  let (_client_1, mut client_2, object_id, mut expected_json) = make_clients().await;
  // "title" => "hello world" is not delivered to client_2 and is considered a missing update
  client_2.enable_receive_message();
  {
    let mut lock = client_2
      .collabs
      .get_mut(&object_id)
      .unwrap()
      .collab
      .write()
      .await;
    lock.insert("content", "hello world");
  }

  expected_json["content"] = Value::String("hello world".to_string());

  // the collab ping will trigger a init sync with reason InitSyncReason::MissUpdates after a period of time
  assert_client_collab_include_value(&mut client_2, &object_id, expected_json)
    .await
    .unwrap();
}

#[tokio::test]
async fn client_ping_find_missing_update_test() {
  let (_client_1, mut client_2, object_id, expected_json) = make_clients().await;
  // "title" => "hello world" is not delivered to client_2 and is considered a missing update
  client_2.enable_receive_message();

  // the collab ping will trigger a init sync with reason InitSyncReason::MissUpdates after a period of time
  assert_client_collab_include_value(&mut client_2, &object_id, expected_json)
    .await
    .unwrap();
}

/// Create two clients and the first client makes an edit to the collaborative document.
/// The second client did do init sync but disable receive message, so it will miss the first edit.
async fn make_clients() -> (TestClient, TestClient, Uuid, Value) {
  let collab_type = CollabType::Unknown;
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;
  // Create a collaborative document with client_1 and invite client_2 to collaborate.
  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .open_and_edit_collab(workspace_id, collab_type)
    .await;
  client_1
    .invite_and_accepted_workspace_member(&workspace_id, &client_2, AFRole::Member)
    .await
    .unwrap();

  // after client 2 finish init sync and then disable receive message
  client_2
    .open_collab(workspace_id, object_id, collab_type)
    .await;
  client_2
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();
  client_2.disable_receive_message();

  // Client_1 makes the first edit by inserting "task 1".
  {
    let mut lock = client_1
      .collabs
      .get_mut(&object_id)
      .unwrap()
      .collab
      .write()
      .await;
    lock.insert("title", "hello world");
  }
  client_1
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();

  sleep(Duration::from_secs(2)).await;
  let expected_json = json!({
    "title": "hello world"
  });
  (client_1, client_2, object_id, expected_json)
}
