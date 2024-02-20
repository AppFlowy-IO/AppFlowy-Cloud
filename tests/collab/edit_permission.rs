use crate::collab::util::generate_random_string;
use assert_json_diff::{assert_json_eq, assert_json_include};
use client_api_test_util::{
  assert_client_collab, assert_client_collab_include_value, assert_server_collab, TestClient,
};
use collab_entity::CollabType;
use database_entity::dto::{AFAccessLevel, AFRole};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

#[tokio::test]
async fn recv_updates_without_permission_test() {
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;

  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  client_2
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  // Edit the collab from client 1 and then the server will broadcast to client 2. But the client 2
  // is not the member of the collab, so the client 2 will not receive the update.
  client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");
  client_1.wait_object_sync_complete(&object_id).await;
  assert_client_collab(&mut client_2, &object_id, "name", json!({}), 3).await;
}

#[tokio::test]
async fn recv_remote_updates_with_readonly_permission_test() {
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;

  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  // Add client 2 as the member of the collab then the client 2 will receive the update.
  client_1
    .add_client_as_collab_member(
      &workspace_id,
      &object_id,
      &client_2,
      AFAccessLevel::ReadOnly,
    )
    .await;

  client_2
    .open_collab(&workspace_id, &object_id, collab_type.clone())
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

  let expected = json!({
    "name": "AppFlowy"
  });
  assert_client_collab(&mut client_2, &object_id, "name", expected.clone(), 10).await;
  assert_server_collab(
    &workspace_id,
    &mut client_1.api_client,
    &object_id,
    &collab_type,
    10,
    expected,
  )
  .await;
}

#[tokio::test]
async fn init_sync_with_readonly_permission_test() {
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;

  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;
  client_1
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");
  client_1.wait_object_sync_complete(&object_id).await;

  //
  let expected = json!({
    "name": "AppFlowy"
  });
  assert_server_collab(
    &workspace_id,
    &mut client_1.api_client,
    &object_id,
    &collab_type,
    10,
    expected.clone(),
  )
  .await;

  // Add client 2 as the member of the collab with readonly permission.
  // client 2 can pull the latest updates via the init sync. But it's not allowed to send local changes.
  client_1
    .add_client_as_collab_member(
      &workspace_id,
      &object_id,
      &client_2,
      AFAccessLevel::ReadOnly,
    )
    .await;
  client_2
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;
  assert_client_collab_include_value(&mut client_2, &object_id, expected).await;
}

#[tokio::test]
async fn edit_collab_with_readonly_permission_test() {
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;

  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  // Add client 2 as the member of the collab then the client 2 will receive the update.
  client_1
    .add_client_as_collab_member(
      &workspace_id,
      &object_id,
      &client_2,
      AFAccessLevel::ReadOnly,
    )
    .await;

  client_2
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  // client 2 edit the collab and then the server will reject the update which mean the
  // collab in the server will not be updated.
  client_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");
  assert_client_collab_include_value(
    &mut client_2,
    &object_id,
    json!({
      "name": "AppFlowy"
    }),
  )
  .await;

  assert_server_collab(
    &workspace_id,
    &mut client_1.api_client,
    &object_id,
    &collab_type,
    5,
    json!({}),
  )
  .await;
}

#[tokio::test]
async fn edit_collab_with_read_and_write_permission_test() {
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;

  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  // Add client 2 as the member of the collab then the client 2 will receive the update.
  client_1
    .add_client_as_collab_member(
      &workspace_id,
      &object_id,
      &client_2,
      AFAccessLevel::ReadAndWrite,
    )
    .await;

  client_2
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  // client 2 edit the collab and then the server will broadcast the update
  client_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");

  let expected = json!({
    "name": "AppFlowy"
  });
  assert_client_collab_include_value(&mut client_2, &object_id, expected.clone()).await;

  assert_server_collab(
    &workspace_id,
    &mut client_1.api_client,
    &object_id,
    &collab_type,
    5,
    expected,
  )
  .await;
}

#[tokio::test]
async fn edit_collab_with_full_access_permission_test() {
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;

  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  // Add client 2 as the member of the collab then the client 2 will receive the update.
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

  // client 2 edit the collab and then the server will broadcast the update
  client_2
    .collab_by_object_id
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");

  let expected = json!({
    "name": "AppFlowy"
  });
  assert_client_collab(&mut client_2, &object_id, "name", expected.clone(), 5).await;

  assert_server_collab(
    &workspace_id,
    &mut client_1.api_client,
    &object_id,
    &collab_type,
    5,
    expected,
  )
  .await;
}

#[tokio::test]
async fn edit_collab_with_full_access_then_readonly_permission() {
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;

  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  // Add client 2 as the member of the collab then the client 2 will receive the update.
  client_1
    .add_client_as_collab_member(
      &workspace_id,
      &object_id,
      &client_2,
      AFAccessLevel::FullAccess,
    )
    .await;

  // client 2 edit the collab and then the server will broadcast the update
  {
    client_2
      .open_collab(&workspace_id, &object_id, collab_type.clone())
      .await;
    client_2
      .collab_by_object_id
      .get_mut(&object_id)
      .unwrap()
      .collab
      .lock()
      .insert("title", "hello world");
    client_2.wait_object_sync_complete(&object_id).await;
  }

  // update the permission from full access to readonly, then the server will reject the subsequent
  // updates generated by client 2
  {
    client_1
      .update_collab_member_access_level(
        &workspace_id,
        &object_id,
        &client_2,
        AFAccessLevel::ReadOnly,
      )
      .await;
    client_2
      .collab_by_object_id
      .get_mut(&object_id)
      .unwrap()
      .collab
      .lock()
      .insert("subtitle", "Writing Rust, fun");
  }

  assert_client_collab_include_value(
    &mut client_2,
    &object_id,
    json!({
      "title": "hello world",
      "subtitle": "Writing Rust, fun"
    }),
  )
  .await;
  assert_server_collab(
    &workspace_id,
    &mut client_1.api_client,
    &object_id,
    &collab_type,
    5,
    json!({
      "title": "hello world"
    }),
  )
  .await;
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
      // sleep 2 secs to make sure it do not trigger register user too fast in gotrue
      sleep(Duration::from_secs(i % 3)).await;

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
      // sleep 2 secs to make sure it do not trigger register user too fast in gotrue
      sleep(Duration::from_secs(i % 2)).await;
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

      // wait 3 seconds to let the client try to send the update to the server
      // can't use want_object_sync_complete because the client do not have permission to send the update
      sleep(Duration::from_secs(3)).await;
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
