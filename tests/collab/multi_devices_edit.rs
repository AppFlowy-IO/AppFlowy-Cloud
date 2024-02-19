use assert_json_diff::{assert_json_eq, assert_json_include};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use client_api_test_util::*;
use collab_entity::CollabType;
use serde_json::json;
use sqlx::types::uuid;
use tokio::time::sleep;
use tracing::trace;
use uuid::Uuid;

use crate::collab::util::generate_random_string;
use database_entity::dto::{AFAccessLevel, AFRole, QueryCollabParams};

#[tokio::test]
async fn edit_collab_with_ws_reconnect_sync_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  test_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

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
    .get_collab(QueryCollabParams::new(
      &object_id,
      collab_type.clone(),
      &workspace_id,
    ))
    .await
    .unwrap_err();
  assert!(err.is_record_not_found());

  // After reconnect the collab should be synced to the server.
  test_client.reconnect().await;
  // Wait for the messages to be sent
  test_client.wait_object_sync_complete(&object_id).await;

  assert_server_collab(
    &workspace_id,
    &mut test_client.api_client,
    &object_id,
    &collab_type,
    10,
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
async fn edit_collab_with_different_devices_test() {
  let collab_type = CollabType::Document;
  let registered_user = generate_unique_registered_user().await;
  let mut client_1 = TestClient::user_with_new_device(registered_user.clone()).await;
  let mut client_2 = TestClient::user_with_new_device(registered_user.clone()).await;

  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  // client 1 edit the collab
  client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "work");
  client_1.wait_object_sync_complete(&object_id).await;

  client_2
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;
  sleep(Duration::from_millis(1000)).await;

  trace!("client 2 disconnect: {:?}", client_2.device_id);
  client_2.disconnect().await;
  sleep(Duration::from_millis(1000)).await;

  client_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "workspace");

  client_2.reconnect().await;
  client_2.wait_object_sync_complete(&object_id).await;

  let expected_json = json!({
    "name": "workspace"
  });
  assert_client_collab(&mut client_1, &object_id, "name", expected_json.clone(), 10).await;
  assert_client_collab(&mut client_2, &object_id, "name", expected_json.clone(), 10).await;
}

#[tokio::test]
async fn edit_document_with_both_clients_offline_then_online_sync_test() {
  let _object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;

  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  // add client 2 as a member of the collab
  client_1
    .add_client_as_collab_member(
      &workspace_id,
      &object_id,
      &client_2,
      AFAccessLevel::ReadAndWrite,
    )
    .await;
  client_1.disconnect().await;

  client_2
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;
  client_2.disconnect().await;

  for i in 0..10 {
    if i % 2 == 0 {
      client_1
        .collab_by_object_id
        .get_mut(&object_id)
        .unwrap()
        .collab
        .lock()
        .insert(&i.to_string(), format!("Task {}", i));
    } else {
      client_2
        .collab_by_object_id
        .get_mut(&object_id)
        .unwrap()
        .collab
        .lock()
        .insert(&i.to_string(), format!("Task {}", i));
    }
  }

  tokio::join!(client_1.reconnect(), client_2.reconnect());
  tokio::join!(
    client_1.wait_object_sync_complete(&object_id),
    client_2.wait_object_sync_complete(&object_id)
  );

  let expected_json = json!({
    "0": "Task 0",
    "1": "Task 1",
    "2": "Task 2",
    "3": "Task 3",
    "4": "Task 4",
    "5": "Task 5",
    "6": "Task 6",
    "7": "Task 7",
    "8": "Task 8",
    "9": "Task 9"
  });
  assert_client_collab_include_value(&mut client_1, &object_id, expected_json.clone()).await;
  assert_client_collab_include_value(&mut client_2, &object_id, expected_json.clone()).await;
}

#[tokio::test]
async fn multiple_user_with_read_and_write_permission_edit_same_collab_test() {
  let mut tasks = Vec::new();
  let mut owner = TestClient::new_user().await;
  let object_id = Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let workspace_id = owner.workspace_id().await;
  owner
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;
  let arc_owner = Arc::new(owner);

  // simulate multiple users edit the same collab. All of them have read and write permission
  for i in 0..10 {
    let owner = arc_owner.clone();
    let object_id = object_id.clone();
    let collab_type = collab_type.clone();
    let workspace_id = workspace_id.clone();
    let task = tokio::spawn(async move {
      let mut new_user = TestClient::new_user().await;
      owner
        .add_workspace_member(&workspace_id, &new_user, AFRole::Member)
        .await;
      owner
        .add_client_as_collab_member(
          &workspace_id,
          &object_id,
          &new_user,
          AFAccessLevel::ReadAndWrite,
        )
        .await;

      new_user
        .open_collab(&workspace_id, &object_id, collab_type.clone())
        .await;

      // generate random string and insert it to the collab
      let random_str = generate_random_string(200);
      new_user
        .collab_by_object_id
        .get_mut(&object_id)
        .unwrap()
        .collab
        .lock()
        .insert(&i.to_string(), random_str.clone());
      new_user.wait_object_sync_complete(&object_id).await;
      (random_str, new_user)
    });
    tasks.push(task);
  }

  let results = futures::future::join_all(tasks).await;
  let mut expected_json = HashMap::new();
  let mut clients = vec![];
  for (index, result) in results.into_iter().enumerate() {
    let (s, client) = result.unwrap();
    clients.push(client);
    expected_json.insert(index.to_string(), s);
  }

  // all the clients should have the same collab object
  assert_json_include!(
    actual: json!(expected_json),
    expected: arc_owner
      .collab_by_object_id
      .get(&object_id)
      .unwrap()
      .collab
      .to_json_value()
  );

  for client in clients {
    assert_json_include!(
      expected: json!(expected_json),
      actual: client
        .collab_by_object_id
        .get(&object_id)
        .unwrap()
        .collab
        .to_json_value()
    );
  }
}

#[tokio::test]
async fn multiple_user_with_read_only_permission_edit_same_collab_test() {
  let mut tasks = Vec::new();
  let mut owner = TestClient::new_user().await;
  let object_id = Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let workspace_id = owner.workspace_id().await;
  owner
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;
  let arc_owner = Arc::new(owner);

  for i in 0..5 {
    let owner = arc_owner.clone();
    let object_id = object_id.clone();
    let collab_type = collab_type.clone();
    let workspace_id = workspace_id.clone();
    let task = tokio::spawn(async move {
      let mut new_user = TestClient::new_user().await;
      owner
        .add_client_as_collab_member(
          &workspace_id,
          &object_id,
          &new_user,
          AFAccessLevel::ReadOnly,
        )
        .await;

      new_user
        .open_collab(&workspace_id, &object_id, collab_type.clone())
        .await;

      let random_str = generate_random_string(200);
      new_user
        .collab_by_object_id
        .get_mut(&object_id)
        .unwrap()
        .collab
        .lock()
        .insert(&i.to_string(), random_str.clone());
      new_user.wait_object_sync_complete(&object_id).await;
      (random_str, new_user)
    });
    tasks.push(task);
  }

  let results = futures::future::join_all(tasks).await;
  for (index, result) in results.into_iter().enumerate() {
    let (s, client) = result.unwrap();
    assert_json_eq!(
      json!({index.to_string(): s}),
      client
        .collab_by_object_id
        .get(&object_id)
        .unwrap()
        .collab
        .to_json_value(),
    );
  }
  // all the clients should have the same collab object
  assert_json_eq!(
    json!({}),
    arc_owner
      .collab_by_object_id
      .get(&object_id)
      .unwrap()
      .collab
      .to_json_value(),
  );
}
