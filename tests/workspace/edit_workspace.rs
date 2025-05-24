use std::time::Duration;

use collab_entity::CollabType;
use serde_json::json;
use tokio::time::sleep;

use client_api_test::*;
use database_entity::dto::AFRole;

#[tokio::test]
async fn init_sync_workspace_with_member_permission() {
  let mut owner = TestClient::new_user().await;
  let mut guest = TestClient::new_user().await;
  let workspace_id = owner.workspace_id().await;
  owner.open_workspace_collab(workspace_id).await;

  // TODO(nathan): write test for AFRole::Guest
  // add client 2 as the member of the workspace then the client 2 will receive the update.
  owner
    .invite_and_accepted_workspace_member(&workspace_id, &guest, AFRole::Member)
    .await
    .unwrap();
  guest.open_workspace_collab(workspace_id).await;

  owner.insert_into(&workspace_id, "name", "AppFlowy").await;
  owner
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();

  assert_client_collab_within_secs(
    &mut owner,
    &workspace_id,
    "name",
    json!({"name": "AppFlowy"}),
    60,
  )
  .await;
  assert_client_collab_within_secs(
    &mut guest,
    &workspace_id,
    "name",
    json!({"name": "AppFlowy"}),
    60,
  )
  .await;
}

#[tokio::test]
async fn edit_workspace_with_guest_permission() {
  let mut owner = TestClient::new_user().await;
  let mut guest = TestClient::new_user().await;
  let workspace_id = owner.workspace_id().await;
  owner.open_workspace_collab(workspace_id).await;

  // add client 2 as the member of the workspace then the client 2 can receive the update.
  owner
    .invite_and_accepted_workspace_member(&workspace_id, &guest, AFRole::Guest)
    .await
    .unwrap();

  owner.insert_into(&workspace_id, "name", "zack").await;
  owner
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();

  guest.open_workspace_collab(workspace_id).await;
  // make sure the client 2 has received the remote updates before the client 2 edits the collab
  sleep(Duration::from_secs(3)).await;

  // client_2 only has the guest permission, so it can not edit the collab
  guest.insert_into(&workspace_id, "name", "nathan").await;

  assert_client_collab_include_value(&mut owner, &workspace_id, json!({"name": "zack"}))
    .await
    .unwrap();
  assert_client_collab_include_value(&mut guest, &workspace_id, json!({"name": "nathan"}))
    .await
    .unwrap();

  assert_server_collab(
    workspace_id,
    &mut owner.api_client,
    workspace_id,
    &CollabType::Folder,
    30,
    json!({
      "name": "zack"
    }),
  )
  .await
  .unwrap();
}
