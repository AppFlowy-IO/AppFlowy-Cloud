use serde_json::json;

use crate::realtime::test_client::{assert_client_collab, assert_remote_collab, TestClient};
use collab_entity::CollabType;
use sqlx::types::uuid;

use std::time::Duration;

#[tokio::test]
async fn realtime_write_single_collab_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.current_workspace_id().await;
  test_client
    .create_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  // Edit the collab
  for i in 0..=5 {
    test_client
      .collab_by_object_id
      .get_mut(&object_id)
      .unwrap()
      .collab
      .lock()
      .insert(&i.to_string(), i.to_string());
  }

  let expected_json = json!( {
    "0": "0",
    "1": "1",
    "2": "2",
    "3": "3",
    "4": "4",
    "5": "5",
  });
  test_client.wait_object_sync_complete(&object_id).await;

  assert_remote_collab(
    &mut test_client.api_client,
    &object_id,
    &collab_type,
    10,
    expected_json,
  )
  .await;
}

#[tokio::test]
async fn realtime_write_multiple_collab_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.current_workspace_id().await;
  let mut object_ids = vec![];
  for _ in 0..5 {
    let object_id = uuid::Uuid::new_v4().to_string();
    let collab_type = CollabType::Document;
    test_client
      .create_collab(&workspace_id, &object_id, collab_type.clone())
      .await;
    for i in 0..=5 {
      test_client
        .collab_by_object_id
        .get_mut(&object_id)
        .unwrap()
        .collab
        .lock()
        .insert(&i.to_string(), i.to_string());
    }

    test_client.wait_object_sync_complete(&object_id).await;
    object_ids.push(object_id);
  }

  // Wait for the messages to be sent
  for object_id in object_ids {
    assert_remote_collab(
      &mut test_client.api_client,
      &object_id,
      &CollabType::Document,
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
}

#[tokio::test]
async fn one_direction_peer_sync_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let mut client_1 = TestClient::new_user().await;
  let workspace_id = client_1.current_workspace_id().await;
  client_1
    .create_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  let mut client_2 = TestClient::new_user().await;
  client_2
    .create_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  // Edit the collab from client 1 and then the server will broadcast to client 2
  client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");
  client_1.wait_object_sync_complete(&object_id).await;

  assert_client_collab(
    &mut client_2,
    &object_id,
    json!({
      "name": "AppFlowy"
    }),
  )
  .await;

  assert_remote_collab(
    &mut client_1.api_client,
    &object_id,
    &collab_type,
    10,
    json!({
      "name": "AppFlowy"
    }),
  )
  .await;
}
//
// #[tokio::test]
// async fn user_with_duplicate_devices_connect_edit_test() {
//   let object_id = uuid::Uuid::new_v4().to_string();
//   let collab_type = CollabType::Document;
//   let registered_user = generate_unique_registered_user().await;
//
//   // Client_1_2 will force the server to disconnect client_1_1. So any changes made by client_1_1
//   // will not be saved to the server.
//   let device_id = Uuid::new_v4().to_string();
//   let mut client_1_1 = TestClient::new(device_id.clone(), registered_user.clone()).await;
//   let workspace_id = client_1_1.current_workspace_id().await;
//
//   client_1_1
//     .create_collab(&workspace_id, &object_id, collab_type.clone())
//     .await;
//
//   client_1_1
//     .collab_by_object_id
//     .get_mut(&object_id)
//     .unwrap()
//     .collab
//     .lock()
//     .insert("1", "a");
//   client_1_1
//     .collab_by_object_id
//     .get_mut(&object_id)
//     .unwrap()
//     .collab
//     .lock()
//     .insert("3", "c");
//   client_1_1.wait_object_sync_complete(&object_id).await;
//
//   let mut client_1_2 = TestClient::new(device_id.clone(), registered_user.clone()).await;
//   client_1_2
//     .create_collab(&workspace_id, &object_id, collab_type.clone())
//     .await;
//   client_1_2
//     .collab_by_object_id
//     .get_mut(&object_id)
//     .unwrap()
//     .collab
//     .lock()
//     .insert("2", "b");
//   client_1_2.wait_object_sync_complete(&object_id).await;
//
//   assert_client_collab(
//     &mut client_1_1,
//     &object_id,
//     10,
//     json!({
//       "1": "a",
//       "3": "c"
//     }),
//   )
//   .await;
//
//   assert_client_collab(
//     &mut client_1_2,
//     &object_id,
//     10,
//     json!({
//       "1": "a",
//       "3": "c",
//       "2": "b"
//     }),
//   )
//   .await;
//
//   assert_remote_collab_json(
//     &mut client_1_2.api_client,
//     &object_id,
//     &collab_type,
//     5,
//     json!({
//       "1": "a",
//       "2": "b",
//       "3": "c"
//     }),
//   )
//   .await;
// }

#[tokio::test]
async fn two_direction_peer_sync_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let mut client_1 = TestClient::new_user().await;
  let workspace_id = client_1.current_workspace_id().await;
  client_1
    .create_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  let mut client_2 = TestClient::new_user().await;
  client_2
    .create_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");
  client_1.wait_object_sync_complete(&object_id).await;

  client_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("support platform", "macOS, Windows, Linux, iOS, Android");
  client_2.wait_object_sync_complete(&object_id).await;

  let expected_json = json!({
    "name": "AppFlowy",
    "support platform": "macOS, Windows, Linux, iOS, Android"
  });
  assert_client_collab(&mut client_1, &object_id, expected_json.clone()).await;
  assert_client_collab(&mut client_2, &object_id, expected_json.clone()).await;
}

#[tokio::test]
async fn client_init_sync_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let mut client_1 = TestClient::new_user().await;
  let workspace_id = client_1.current_workspace_id().await;
  client_1
    .create_collab(&workspace_id, &object_id, collab_type.clone())
    .await;
  client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");
  client_1.wait_object_sync_complete(&object_id).await;
  tokio::time::sleep(Duration::from_millis(1000)).await;

  let mut client_2 = TestClient::new_user().await;
  client_2
    .create_collab(&workspace_id, &object_id, collab_type.clone())
    .await;
  client_2.wait_object_sync_complete(&object_id).await;

  let expected_json = json!({
    "name": "AppFlowy",
  });
  assert_client_collab(&mut client_1, &object_id, expected_json.clone()).await;
  assert_client_collab(&mut client_2, &object_id, expected_json.clone()).await;
}

#[tokio::test]
async fn multiple_collab_edit_test() {
  let collab_type = CollabType::Document;
  let object_id_1 = uuid::Uuid::new_v4().to_string();
  let mut client_1 = TestClient::new_user().await;
  let workspace_id = client_1.current_workspace_id().await;
  client_1
    .create_collab(&workspace_id, &object_id_1, collab_type.clone())
    .await;

  let object_id_2 = uuid::Uuid::new_v4().to_string();
  let mut client_2 = TestClient::new_user().await;
  client_2
    .create_collab(&workspace_id, &object_id_2, collab_type.clone())
    .await;

  let object_id_3 = uuid::Uuid::new_v4().to_string();
  let mut client_3 = TestClient::new_user().await;
  client_3
    .create_collab(&workspace_id, &object_id_3, collab_type.clone())
    .await;

  client_1
    .collab_by_object_id
    .get_mut(&object_id_1)
    .unwrap()
    .collab
    .lock()
    .insert("title", "I am client 1");
  client_1.wait_object_sync_complete(&object_id_1).await;
  client_2
    .collab_by_object_id
    .get_mut(&object_id_2)
    .unwrap()
    .collab
    .lock()
    .insert("title", "I am client 2");
  client_2.wait_object_sync_complete(&object_id_2).await;
  client_3
    .collab_by_object_id
    .get_mut(&object_id_3)
    .unwrap()
    .collab
    .lock()
    .insert("title", "I am client 3");
  client_3.wait_object_sync_complete(&object_id_3).await;

  assert_remote_collab(
    &mut client_1.api_client,
    &object_id_1,
    &collab_type,
    10,
    json!( {
      "title": "I am client 1"
    }),
  )
  .await;

  assert_remote_collab(
    &mut client_2.api_client,
    &object_id_2,
    &collab_type,
    10,
    json!( {
      "title": "I am client 2"
    }),
  )
  .await;
  assert_remote_collab(
    &mut client_3.api_client,
    &object_id_3,
    &collab_type,
    10,
    json!( {
      "title": "I am client 3"
    }),
  )
  .await;
}
