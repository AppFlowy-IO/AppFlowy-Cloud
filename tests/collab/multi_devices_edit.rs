use client_api_test_util::*;
use collab_entity::CollabType;
use database_entity::dto::{AFAccessLevel, QueryCollabParams};
use serde_json::json;
use sqlx::types::uuid;

#[tokio::test]
async fn sync_collab_content_after_reconnect_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Unknown;

  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  test_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  // Disconnect the client and edit the collab. The updates will not be sent to the server.
  test_client.disconnect().await;
  for i in 0..=5 {
    test_client
      .collabs
      .get_mut(&object_id)
      .unwrap()
      .mutex_collab
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
  test_client
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();

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
  .await
  .unwrap();
}

// #[tokio::test]
// async fn same_client_with_diff_devices_edit_same_collab_test() {
//   let collab_type = CollabType::Unknown;
//   let registered_user = generate_unique_registered_user().await;
//   let mut client_1 = TestClient::user_with_new_device(registered_user.clone()).await;
//   let mut client_2 = TestClient::user_with_new_device(registered_user.clone()).await;
//
//   let workspace_id = client_1.workspace_id().await;
//   let object_id = client_1
//     .create_and_edit_collab(&workspace_id, collab_type.clone())
//     .await;
//
//   // client 1 edit the collab
//   client_1
//     .collabs
//     .get_mut(&object_id)
//     .unwrap()
//     .mutex_collab
//     .lock()
//     .insert("name", "workspace1");
//   client_1
//     .wait_object_sync_complete(&object_id)
//     .await
//     .unwrap();
//
//   client_2
//     .open_collab(&workspace_id, &object_id, collab_type.clone())
//     .await;
//   client_2
//     .wait_object_sync_complete(&object_id)
//     .await
//     .unwrap();
//   trace!("client 2 disconnect: {:?}", client_2.device_id);
//   client_2.disconnect().await;
//   sleep(Duration::from_millis(2000)).await;
//
//   client_2
//     .collabs
//     .get_mut(&object_id)
//     .unwrap()
//     .mutex_collab
//     .lock()
//     .insert("name", "workspace2");
//   client_2.reconnect().await;
//   client_2
//     .wait_object_sync_complete(&object_id)
//     .await
//     .unwrap();
//
//   let expected_json = json!({
//     "name": "workspace2"
//   });
//
//   assert_client_collab_within_secs(&mut client_2, &object_id, "name", expected_json.clone(), 60)
//     .await;
//   assert_client_collab_within_secs(&mut client_1, &object_id, "name", expected_json.clone(), 60)
//     .await;
// }

#[tokio::test]
async fn same_client_with_diff_devices_edit_diff_collab_test() {
  let registered_user = generate_unique_registered_user().await;
  let collab_type = CollabType::Unknown;
  let mut device_1 = TestClient::user_with_new_device(registered_user.clone()).await;
  let mut device_2 = TestClient::user_with_new_device(registered_user.clone()).await;

  let workspace_id = device_1.workspace_id().await;

  // different devices create different collabs. the collab will be synced between devices
  let object_id_1 = device_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;
  let object_id_2 = device_2
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  // client 1 edit the collab with object_id_1
  device_1
    .collabs
    .get_mut(&object_id_1)
    .unwrap()
    .mutex_collab
    .lock()
    .insert("name", "object 1");
  device_1
    .wait_object_sync_complete(&object_id_1)
    .await
    .unwrap();

  // client 2 edit the collab with object_id_2
  device_2
    .collabs
    .get_mut(&object_id_2)
    .unwrap()
    .mutex_collab
    .lock()
    .insert("name", "object 2");
  device_2
    .wait_object_sync_complete(&object_id_2)
    .await
    .unwrap();

  // client1 open the collab with object_id_2
  device_1
    .open_collab(&workspace_id, &object_id_2, collab_type.clone())
    .await;
  assert_client_collab_within_secs(
    &mut device_1,
    &object_id_2,
    "name",
    json!({
      "name": "object 2"
    }),
    60,
  )
  .await;

  // client2 open the collab with object_id_1
  device_2
    .open_collab(&workspace_id, &object_id_1, collab_type.clone())
    .await;
  assert_client_collab_within_secs(
    &mut device_2,
    &object_id_1,
    "name",
    json!({
      "name": "object 1"
    }),
    60,
  )
  .await;
}

#[tokio::test]
async fn edit_document_with_both_clients_offline_then_online_sync_test() {
  let collab_type = CollabType::Unknown;
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;

  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  // add client 2 as a member of the collab
  client_1
    .add_collab_member(
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
        .collabs
        .get_mut(&object_id)
        .unwrap()
        .mutex_collab
        .lock()
        .insert(&i.to_string(), format!("Task {}", i));
    } else {
      client_2
        .collabs
        .get_mut(&object_id)
        .unwrap()
        .mutex_collab
        .lock()
        .insert(&i.to_string(), format!("Task {}", i));
    }
  }

  tokio::join!(client_1.reconnect(), client_2.reconnect());
  let (left, right) = tokio::join!(
    client_1.wait_object_sync_complete(&object_id),
    client_2.wait_object_sync_complete(&object_id)
  );
  assert!(left.is_ok());
  assert!(right.is_ok());

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
  assert_client_collab_include_value(&mut client_1, &object_id, expected_json.clone())
    .await
    .unwrap();
  assert_client_collab_include_value(&mut client_2, &object_id, expected_json.clone())
    .await
    .unwrap();
}
