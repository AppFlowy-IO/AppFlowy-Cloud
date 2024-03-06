use app_error::ErrorCode;
use client_api_test_util::{api_client_with_email, TestClient};
use database_entity::dto::{AFAccessLevel, AFRole, QueryCollabMembers};
use shared_entity::dto::workspace_dto::WorkspaceMemberInvitation;

#[tokio::test]
async fn get_workspace_owner_after_sign_up_test() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = c1.workspace_id().await;

  // after the user sign up, the user should be the owner of the workspace
  let members = c1
    .api_client
    .get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members.len(), 1);
  assert_eq!(members[0].email, c1.email().await);

  // after user sign up, the user should have full access to the workspace
  let collab_members = c1
    .api_client
    .get_collab_members(QueryCollabMembers {
      workspace_id: workspace_id.clone(),
      object_id: workspace_id.clone(),
    })
    .await
    .unwrap()
    .0;
  assert_eq!(collab_members.len(), 1);
  assert_eq!(
    collab_members[0].permission.access_level,
    AFAccessLevel::FullAccess
  );
}

#[tokio::test]
async fn add_workspace_members_not_enough_permission() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let member_1 = TestClient::new_user_without_ws_conn().await;
  let member_2 = TestClient::new_user_without_ws_conn().await;

  let workspace_id = owner.workspace_id().await;

  // add client 2 to client 1's workspace
  owner
    .add_workspace_member(&workspace_id, &member_1, AFRole::Member)
    .await;

  // client 2 add client 3 to client 1's workspace but permission denied
  let error = member_1
    .invite_and_accepted_workspace_member(&workspace_id, &member_2, AFRole::Member)
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
  c1.api_client
    .invite_workspace_members(
      &workspace_id,
      vec![WorkspaceMemberInvitation {
        email: email.clone(),
        role: AFRole::Member,
      }],
    )
    .await
    .unwrap();

  let invited_client = api_client_with_email(&email).await;
  let invite_id = invited_client
    .list_workspace_invitations(None)
    .await
    .unwrap()
    .first()
    .unwrap()
    .invite_id;
  invited_client
    .accept_workspace_invitation(invite_id.to_string().as_str())
    .await
    .unwrap();

  let workspaces = invited_client.get_workspaces().await.unwrap();
  assert_eq!(workspaces.0.len(), 2);
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
async fn workspace_add_member() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let other_owner = TestClient::new_user_without_ws_conn().await;
  let member = TestClient::new_user_without_ws_conn().await;
  let guest = TestClient::new_user_without_ws_conn().await;

  let workspace_id = owner.workspace_id().await;

  // add client 2 to client 1's workspace
  owner
    .add_workspace_member(&workspace_id, &other_owner, AFRole::Owner)
    .await;

  // add client 3 to client 1's workspace
  other_owner
    .add_workspace_member(&workspace_id, &member, AFRole::Member)
    .await;
  other_owner
    .add_workspace_member(&workspace_id, &guest, AFRole::Guest)
    .await;

  let members = owner
    .api_client
    .get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members.len(), 4);
  assert_eq!(members[0].email, owner.email().await);
  assert_eq!(members[0].role, AFRole::Owner);

  assert_eq!(members[1].email, other_owner.email().await);
  assert_eq!(members[1].role, AFRole::Owner);

  assert_eq!(members[2].email, member.email().await);
  assert_eq!(members[2].role, AFRole::Member);

  assert_eq!(members[3].email, guest.email().await);
  assert_eq!(members[3].role, AFRole::Guest);

  // after adding the members to the workspace, we should be able to get the collab members
  // of the workspace.
  let collab_members = owner
    .api_client
    .get_collab_members(QueryCollabMembers {
      workspace_id: workspace_id.clone(),
      object_id: workspace_id.clone(),
    })
    .await
    .unwrap()
    .0;

  assert_eq!(collab_members.len(), 4);

  // owner
  assert_eq!(collab_members[0].uid, owner.uid().await);
  assert_eq!(
    collab_members[0].permission.access_level,
    AFAccessLevel::FullAccess
  );

  // other owner
  assert_eq!(collab_members[1].uid, other_owner.uid().await);
  assert_eq!(
    collab_members[1].permission.access_level,
    AFAccessLevel::FullAccess
  );

  // member
  assert_eq!(collab_members[2].uid, member.uid().await);
  assert_eq!(
    collab_members[2].permission.access_level,
    AFAccessLevel::ReadAndWrite
  );

  // guest
  assert_eq!(collab_members[3].uid, guest.uid().await);
  assert_eq!(
    collab_members[3].permission.access_level,
    AFAccessLevel::ReadOnly
  );
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
