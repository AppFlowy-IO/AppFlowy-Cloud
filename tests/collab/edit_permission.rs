use crate::util::test_client::{
  assert_client_collab, assert_client_collab_include_value, assert_server_collab, TestClient,
};
use collab_entity::CollabType;
use database_entity::dto::AFAccessLevel;
use serde_json::json;

#[tokio::test]
async fn recv_updates_without_permission_test() {
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;

  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_collab(&workspace_id, collab_type.clone())
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
    .create_collab(&workspace_id, collab_type.clone())
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
    .create_collab(&workspace_id, collab_type.clone())
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
    .create_collab(&workspace_id, collab_type.clone())
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
    .create_collab(&workspace_id, collab_type.clone())
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
    .create_collab(&workspace_id, collab_type.clone())
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
    .create_collab(&workspace_id, collab_type.clone())
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
