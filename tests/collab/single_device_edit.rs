use crate::util::test_client::{
  assert_client_collab_include_value, assert_server_collab, TestClient,
};
use collab_entity::CollabType;

use database_entity::dto::AFAccessLevel;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn collab_storage_plugin_write_test() {
  let collab_type = CollabType::Document;
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4().to_string();

  // Calling the open_collab function directly will create the collab object in the plugin.
  // The [CollabStoragePlugin] plugin try to get the collab object from the database, but it doesn't exist.
  // So the plugin will create the collab object.
  test_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
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
  test_client.wait_object_sync_complete(&object_id).await;
  test_client.disconnect().await;

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
async fn realtime_write_single_collab_test() {
  let collab_type = CollabType::Document;
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = test_client
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;
  test_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
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

  assert_server_collab(
    &workspace_id,
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
  let workspace_id = test_client.workspace_id().await;
  let mut object_ids = vec![];
  for _ in 0..5 {
    let collab_type = CollabType::Document;

    let object_id = test_client
      .create_and_edit_collab(&workspace_id, collab_type.clone())
      .await;

    test_client
      .open_collab(&workspace_id, &object_id, collab_type.clone())
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
    assert_server_collab(
      &workspace_id,
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

//
#[tokio::test]
async fn user_with_duplicate_devices_connect_edit_test() {
  let collab_type = CollabType::Document;
  let mut old_client = TestClient::new_user().await;
  let workspace_id = old_client.workspace_id().await;

  let object_id = old_client
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  old_client
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("1", "a");
  old_client
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("3", "c");
  old_client.wait_object_sync_complete(&object_id).await;

  // The new_client will receive the old_client's edit
  // The doc will be json!({
  //   "1": "a",
  //   "3": "c"
  // })
  let mut new_client =
    TestClient::new(old_client.device_id.clone(), old_client.user.clone(), true).await;
  new_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;
  new_client
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("2", "b");
  new_client.wait_object_sync_complete(&object_id).await;

  // Old client shouldn't receive the new client's edit
  assert_client_collab_include_value(
    &mut old_client,
    &object_id,
    json!({
      "1": "a",
      "3": "c"
    }),
  )
  .await;

  assert_client_collab_include_value(
    &mut new_client,
    &object_id,
    json!({
      "1": "a",
      "3": "c",
      "2": "b"
    }),
  )
  .await;

  assert_server_collab(
    &workspace_id,
    &mut new_client.api_client,
    &object_id,
    &collab_type,
    10,
    json!({
      "1": "a",
      "2": "b",
      "3": "c"
    }),
  )
  .await;
}

#[tokio::test]
async fn two_direction_peer_sync_test() {
  let collab_type = CollabType::Document;

  let mut client_1 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  let mut client_2 = TestClient::new_user().await;
  // Before the client_2 want to edit the collab object, it needs to become a member of the collab
  // Otherwise, the server will reject the edit request
  client_1
    .add_client_as_collab_member(
      &workspace_id,
      &object_id,
      &client_2,
      AFAccessLevel::FullAccess,
    )
    .await;

  client_2
    .open_collab(&workspace_id, &object_id, collab_type.clone())
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
  assert_client_collab_include_value(&mut client_1, &object_id, expected_json.clone()).await;
  assert_client_collab_include_value(&mut client_2, &object_id, expected_json.clone()).await;
}

#[tokio::test]
async fn multiple_collab_edit_test() {
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new_user().await;
  let workspace_id_1 = client_1.workspace_id().await;
  let object_id_1 = client_1
    .create_and_edit_collab(&workspace_id_1, collab_type.clone())
    .await;
  client_1
    .open_collab(&workspace_id_1, &object_id_1, collab_type.clone())
    .await;

  let mut client_2 = TestClient::new_user().await;
  let workspace_id_2 = client_2.workspace_id().await;
  let object_id_2 = client_2
    .create_and_edit_collab(&workspace_id_2, collab_type.clone())
    .await;
  client_2
    .open_collab(&workspace_id_2, &object_id_2, collab_type.clone())
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

  assert_server_collab(
    &workspace_id_1,
    &mut client_1.api_client,
    &object_id_1,
    &collab_type,
    10,
    json!( {
      "title": "I am client 1"
    }),
  )
  .await;

  assert_server_collab(
    &workspace_id_2,
    &mut client_2.api_client,
    &object_id_2,
    &collab_type,
    10,
    json!( {
      "title": "I am client 2"
    }),
  )
  .await;
}
