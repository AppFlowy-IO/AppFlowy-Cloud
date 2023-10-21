use crate::util::test_client::TestClient;
use database_entity::AFRole;
use shared_entity::error_code::ErrorCode;

#[tokio::test]
async fn add_workspace_members_not_enough_permission() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let c2 = TestClient::new_user_without_ws_conn().await;
  let c3 = TestClient::new_user_without_ws_conn().await;

  let workspace_id = c1.workspace_id().await;

  // add client 2 to client 1's workspace
  c1.add_workspace_member(&workspace_id, &c2, AFRole::Member)
    .await;

  // client 2 add client 3 to client 1's workspace but permission denied
  let error = c2
    .try_add_workspace_member(&workspace_id, &c3, AFRole::Member)
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::NotEnoughPermissions);
}

#[tokio::test]
async fn add_duplicate_workspace_members() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let c2 = TestClient::new_user_without_ws_conn().await;

  let workspace_id = c1.workspace_id().await;

  c1.add_workspace_member(&workspace_id, &c2, AFRole::Member)
    .await;
  c1.add_workspace_member(&workspace_id, &c2, AFRole::Member)
    .await;
}
#[tokio::test]
async fn update_workspace_member_role_not_enough_permission() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let c2 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = c1.workspace_id().await;

  // add client 2 to client 1's workspace
  c1.add_workspace_member(&workspace_id, &c2, AFRole::Member)
    .await;

  // client 2 want to update client 2's role to owner
  let error = c2
    .try_update_workspace_member(&workspace_id, &c2, AFRole::Owner)
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::NotEnoughPermissions);
}

#[tokio::test]
async fn update_workspace_member_role_from_guest_to_member() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let c2 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = c1.workspace_id().await;

  // add client 2 to client 1's workspace
  c1.add_workspace_member(&workspace_id, &c2, AFRole::Guest)
    .await;
  let members = c1
    .api_client
    .get_workspace_members2(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members[0].email, c1.email().await);
  assert_eq!(members[0].role, AFRole::Owner);
  assert_eq!(members[1].email, c2.email().await);
  assert_eq!(members[1].role, AFRole::Guest);

  c1.try_update_workspace_member(&workspace_id, &c2, AFRole::Member)
    .await
    .unwrap();
  let members = c1
    .api_client
    .get_workspace_members2(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members[0].email, c1.email().await);
  assert_eq!(members[0].role, AFRole::Owner);
  assert_eq!(members[1].email, c2.email().await);
  assert_eq!(members[1].role, AFRole::Member);
}

#[tokio::test]
async fn workspace_second_owner_add_member() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let c2 = TestClient::new_user_without_ws_conn().await;
  let c3 = TestClient::new_user_without_ws_conn().await;

  let workspace_id = c1.workspace_id().await;

  // add client 2 to client 1's workspace
  c1.add_workspace_member(&workspace_id, &c2, AFRole::Owner)
    .await;

  // add client 3 to client 1's workspace
  c2.add_workspace_member(&workspace_id, &c3, AFRole::Member)
    .await;

  let members = c1
    .api_client
    .get_workspace_members2(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members.len(), 3);
  assert_eq!(members[0].email, c1.email().await);
  assert_eq!(members[0].role, AFRole::Owner);

  assert_eq!(members[1].email, c2.email().await);
  assert_eq!(members[1].role, AFRole::Owner);

  assert_eq!(members[2].email, c3.email().await);
  assert_eq!(members[2].role, AFRole::Member);
}

#[tokio::test]
async fn add_workspace_member_and_owner_then_delete_all() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let c2 = TestClient::new_user_without_ws_conn().await;
  let c3 = TestClient::new_user_without_ws_conn().await;

  let workspace_id = c1.workspace_id().await;

  // add client 2 to client 1's workspace
  c1.add_workspace_member(&workspace_id, &c2, AFRole::Member)
    .await;
  c1.add_workspace_member(&workspace_id, &c3, AFRole::Owner)
    .await;

  let members = c1
    .api_client
    .get_workspace_members2(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members[0].email, c1.email().await);
  assert_eq!(members[1].email, c2.email().await);
  assert_eq!(members[2].email, c3.email().await);

  // delete the members
  c1.try_remove_workspace_member(&workspace_id, &c2)
    .await
    .unwrap();
  c1.try_remove_workspace_member(&workspace_id, &c3)
    .await
    .unwrap();
  let members = c1
    .api_client
    .get_workspace_members2(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members.len(), 1);
  assert_eq!(members[0].email, c1.email().await);
}

#[tokio::test]
async fn workspace_second_owner_can_not_delete_origin_owner() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let c2 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = c1.workspace_id().await;
  c1.add_workspace_member(&workspace_id, &c2, AFRole::Owner)
    .await;

  let error = c2
    .try_remove_workspace_member(&workspace_id, &c1)
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::NotEnoughPermissions);
}
