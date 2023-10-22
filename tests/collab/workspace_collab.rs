use crate::util::test_client::{assert_client_collab, assert_server_collab, TestClient};
use collab_entity::CollabType;

use database_entity::dto::AFRole;
use serde_json::json;

#[tokio::test]
async fn edit_workspace_without_permission() {
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;

  let workspace_id = client_1.workspace_id().await;
  client_1.open_workspace(&workspace_id).await;
  client_2.open_workspace(&workspace_id).await;

  client_1
    .collab_by_object_id
    .get_mut(&workspace_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");
  client_1.wait_object_sync_complete(&workspace_id).await;

  assert_client_collab(&mut client_1, &workspace_id, json!({"name": "AppFlowy"}), 3).await;
  assert_client_collab(&mut client_2, &workspace_id, json!({}), 3).await;
}

#[tokio::test]
async fn init_sync_workspace_with_guest_permission() {
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;
  client_1.open_workspace(&workspace_id).await;

  // add client 2 as the member of the workspace then the client 2 will receive the update.
  client_1
    .add_workspace_member(&workspace_id, &client_2, AFRole::Guest)
    .await;
  client_2.open_workspace(&workspace_id).await;

  client_1
    .collab_by_object_id
    .get_mut(&workspace_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");
  client_1.wait_object_sync_complete(&workspace_id).await;

  assert_client_collab(&mut client_1, &workspace_id, json!({"name": "AppFlowy"}), 3).await;
  assert_client_collab(&mut client_2, &workspace_id, json!({"name": "AppFlowy"}), 3).await;
}

#[tokio::test]
async fn edit_workspace_with_guest_permission() {
  let mut client_1 = TestClient::new_user().await;
  let mut client_2 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;
  client_1.open_workspace(&workspace_id).await;

  // add client 2 as the member of the workspace then the client 2 will receive the update.
  client_1
    .add_workspace_member(&workspace_id, &client_2, AFRole::Guest)
    .await;

  client_1
    .collab_by_object_id
    .get_mut(&workspace_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "zack");
  client_1.wait_object_sync_complete(&workspace_id).await;

  client_2.open_workspace(&workspace_id).await;
  // make sure the client 2 has received the remote updates before the client 2 edits the collab
  client_2
    .wait_object_sync_complete_with_secs(&workspace_id, 10)
    .await;
  client_2
    .collab_by_object_id
    .get_mut(&workspace_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "nathan");
  client_2
    .wait_object_sync_complete_with_secs(&workspace_id, 5)
    .await;

  assert_client_collab(&mut client_1, &workspace_id, json!({"name": "zack"}), 3).await;
  assert_client_collab(&mut client_2, &workspace_id, json!({"name": "nathan"}), 3).await;

  assert_server_collab(
    &workspace_id,
    &mut client_1.api_client,
    &workspace_id,
    &CollabType::Folder,
    5,
    json!({
      "name": "zack"
    }),
  )
  .await;
}
