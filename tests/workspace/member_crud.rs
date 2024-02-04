use app_error::ErrorCode;
use client_api_test_util::TestClient;
use database_entity::dto::AFRole;
use shared_entity::dto::workspace_dto::CreateWorkspaceMember;

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
async fn add_not_exist_workspace_members() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = c1.workspace_id().await;
  let email = format!("{}@appflowy.io", uuid::Uuid::new_v4());
  let err = c1
    .api_client
    .add_workspace_members(
      workspace_id,
      vec![CreateWorkspaceMember {
        email,
        role: AFRole::Member,
      }],
    )
    .await
    .unwrap_err();

  assert_eq!(err.code, ErrorCode::RecordNotFound);
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
    .get_workspace_members(&workspace_id)
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
    .get_workspace_members(&workspace_id)
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
    .get_workspace_members(&workspace_id)
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
    .get_workspace_members(&workspace_id)
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
    .get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members.len(), 1);
  assert_eq!(members[0].email, c1.email().await);
}

#[tokio::test]
async fn workspace_owner_remove_self_from_workspace() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = c1.workspace_id().await;

  // the workspace owner can not remove 'self' from the workspace
  let error = c1
    .try_remove_workspace_member(&workspace_id, &c1)
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::NotEnoughPermissions);

  let members = c1.get_workspace_members(&workspace_id).await;
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

#[tokio::test]
async fn user_workspace_info() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = c1.workspace_id().await;
  let info = c1.get_user_workspace_info().await;
  assert_eq!(info.workspaces.len(), 1);
  assert_eq!(
    info.visiting_workspace.workspace_id.to_string(),
    workspace_id
  );

  let c2 = TestClient::new_user_without_ws_conn().await;
  c1.add_workspace_member(&workspace_id, &c2, AFRole::Owner)
    .await;

  // c2 should have 2 workspaces
  let info = c2.get_user_workspace_info().await;
  assert_eq!(info.workspaces.len(), 2);
}

#[tokio::test]
async fn get_user_workspace_info_after_open_workspace() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let workspace_id_c1 = c1.workspace_id().await;

  let c2 = TestClient::new_user_without_ws_conn().await;
  c1.add_workspace_member(&workspace_id_c1, &c2, AFRole::Owner)
    .await;

  let info = c2.get_user_workspace_info().await;
  let workspace_id_c2 = c1.workspace_id().await;
  assert_eq!(
    info.visiting_workspace.workspace_id.to_string(),
    workspace_id_c2
  );

  // After open workspace, the visiting workspace should be the workspace that user just opened
  c2.open_workspace(&workspace_id_c1).await;
  let info = c2.get_user_workspace_info().await;
  assert_eq!(
    info.visiting_workspace.workspace_id.to_string(),
    workspace_id_c1
  );
}
