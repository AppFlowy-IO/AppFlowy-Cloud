use app_error::ErrorCode;
use client_api::entity::AFWorkspaceInvitationStatus;
use client_api_test::{api_client_with_email, TestClient};
use database_entity::dto::AFRole;
use shared_entity::dto::workspace_dto::WorkspaceMemberInvitation;

#[tokio::test]
async fn get_workspace_owner_after_sign_up_test() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = c1.workspace_id().await;

  let members = c1
    .api_client
    .get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members.len(), 1);
  assert_eq!(members[0].email, c1.email().await);
}

#[tokio::test]
async fn workspace_members_through_invite_or_direct_add() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let member_1 = TestClient::new_user_without_ws_conn().await;
  let member_2 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = owner.workspace_id().await;
  owner
    .invite_and_accepted_workspace_member(&workspace_id, &member_1, AFRole::Member)
    .await
    .unwrap();

  // TODO(Zack): fix { code: OAuthError, message: "code: 500, msg:Error sending magic link, error_id: Some(\"3ec69543-e7b9-496d-92d8-f0b73ff09e0f\")" }
  owner
    .invite_and_accepted_workspace_member(&workspace_id, &member_2, AFRole::Member)
    .await
    .unwrap();

  let members = owner
    .api_client
    .get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members.len(), 3);
}

#[tokio::test]
async fn add_workspace_members_not_enough_permission() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let member_1 = TestClient::new_user_without_ws_conn().await;
  let member_2 = TestClient::new_user_without_ws_conn().await;

  let workspace_id = owner.workspace_id().await;

  // add client 2 to client 1's workspace
  owner
    .invite_and_accepted_workspace_member(&workspace_id, &member_1, AFRole::Member)
    .await
    .unwrap();

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

  c1.invite_and_accepted_workspace_member(&workspace_id, &c2, AFRole::Member)
    .await
    .unwrap();

  // next invite should return error since the user is already in the workspace
  let err = c1
    .api_client
    .invite_workspace_members(
      &workspace_id,
      vec![WorkspaceMemberInvitation {
        email: c2.email().await,
        role: AFRole::Member,
        skip_email_send: true,
        ..Default::default()
      }],
    )
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::InvalidRequest, "{:?}", err);

  // should not find any invitation
  let invitations = c2
    .api_client
    .list_workspace_invitations(Some(AFWorkspaceInvitationStatus::Pending))
    .await
    .unwrap();

  let is_none = !invitations
    .iter()
    .any(|inv| inv.workspace_id == workspace_id);
  assert!(is_none);
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
        skip_email_send: true,
        ..Default::default()
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
  assert_eq!(workspaces.len(), 2);
}

#[tokio::test]
async fn update_workspace_member_role_not_enough_permission() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let c2 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = c1.workspace_id().await;

  // add client 2 to client 1's workspace
  c1.invite_and_accepted_workspace_member(&workspace_id, &c2, AFRole::Member)
    .await
    .unwrap();

  // client 2 want to update client 2's role to owner
  let error = c2
    .try_update_workspace_member(&workspace_id, &c2, AFRole::Owner)
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::NotEnoughPermissions);
}

#[tokio::test]
async fn update_workspace_member_role_from_guest_to_member() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let guest = TestClient::new_user_without_ws_conn().await;
  let workspace_id = owner.workspace_id().await;

  // add client 2 to client 1's workspace
  owner
    .invite_and_accepted_workspace_member(&workspace_id, &guest, AFRole::Guest)
    .await
    .unwrap();
  let members = owner
    .api_client
    .get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members.len(), 1);
  assert_eq!(members[0].email, owner.email().await);
  assert_eq!(members[0].role, AFRole::Owner);

  owner
    .try_update_workspace_member(&workspace_id, &guest, AFRole::Member)
    .await
    .unwrap();
  let members = owner
    .api_client
    .get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members[0].email, owner.email().await);
  assert_eq!(members[0].role, AFRole::Owner);
  assert_eq!(members[1].email, guest.email().await);
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
    .invite_and_accepted_workspace_member(&workspace_id, &other_owner, AFRole::Owner)
    .await
    .unwrap();

  // add client 3 to client 1's workspace
  other_owner
    .invite_and_accepted_workspace_member(&workspace_id, &member, AFRole::Member)
    .await
    .unwrap();
  other_owner
    .invite_and_accepted_workspace_member(&workspace_id, &guest, AFRole::Guest)
    .await
    .unwrap();

  let members = owner
    .api_client
    .get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members.len(), 3);
  assert_eq!(members[0].email, owner.email().await);
  assert_eq!(members[0].role, AFRole::Owner);

  assert_eq!(members[1].email, other_owner.email().await);
  assert_eq!(members[1].role, AFRole::Owner);

  assert_eq!(members[2].email, member.email().await);
  assert_eq!(members[2].role, AFRole::Member);
}

#[tokio::test]
async fn add_workspace_member_and_owner_then_delete_all() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let member = TestClient::new_user_without_ws_conn().await;
  let second_owner = TestClient::new_user_without_ws_conn().await;

  let workspace_id = owner.workspace_id().await;
  // add client 2 to client 1's workspace
  owner
    .invite_and_accepted_workspace_member(&workspace_id, &member, AFRole::Member)
    .await
    .unwrap();
  owner
    .invite_and_accepted_workspace_member(&workspace_id, &second_owner, AFRole::Owner)
    .await
    .unwrap();

  let members = owner
    .api_client
    .get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members[0].email, owner.email().await);
  assert_eq!(members[1].email, member.email().await);
  assert_eq!(members[2].email, second_owner.email().await);

  // delete the members
  owner
    .try_remove_workspace_member(&workspace_id, &member)
    .await
    .unwrap();
  owner
    .try_remove_workspace_member(&workspace_id, &second_owner)
    .await
    .unwrap();
  let members = owner
    .api_client
    .get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members.len(), 1);
  assert_eq!(members[0].email, owner.email().await);
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
  c1.invite_and_accepted_workspace_member(&workspace_id, &c2, AFRole::Owner)
    .await
    .unwrap();

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
  assert_eq!(info.visiting_workspace.workspace_id, workspace_id);

  let c2 = TestClient::new_user_without_ws_conn().await;
  c1.invite_and_accepted_workspace_member(&workspace_id, &c2, AFRole::Owner)
    .await
    .unwrap();

  // c2 should have 2 workspaces
  let info = c2.get_user_workspace_info().await;
  assert_eq!(info.workspaces.len(), 2);
}

#[tokio::test]
async fn get_user_workspace_info_after_open_workspace() {
  let c1 = TestClient::new_user_without_ws_conn().await;
  let workspace_id_c1 = c1.workspace_id().await;

  let c2 = TestClient::new_user_without_ws_conn().await;
  c1.invite_and_accepted_workspace_member(&workspace_id_c1, &c2, AFRole::Owner)
    .await
    .unwrap();

  let info = c2.get_user_workspace_info().await;
  let workspace_id_c2 = c1.workspace_id().await;
  assert_eq!(info.visiting_workspace.workspace_id, workspace_id_c2);

  // After open workspace, the visiting workspace should be the workspace that user just opened
  c2.open_workspace(&workspace_id_c1).await;
  let info = c2.get_user_workspace_info().await;
  assert_eq!(info.visiting_workspace.workspace_id, workspace_id_c1);
}

#[tokio::test]
async fn member_leave_workspace_test() {
  let c1 = TestClient::new_user().await;
  let workspace_id_c1 = c1.workspace_id().await;

  let c2 = TestClient::new_user().await;
  c1.invite_and_accepted_workspace_member(&workspace_id_c1, &c2, AFRole::Member)
    .await
    .unwrap();
  c2.api_client
    .leave_workspace(&workspace_id_c1)
    .await
    .unwrap();

  let members = c1.get_workspace_members(&workspace_id_c1).await;
  assert_eq!(members.len(), 1);
}

#[tokio::test]
async fn owner_leave_workspace_test() {
  let c1 = TestClient::new_user().await;
  let workspace_id_c1 = c1.workspace_id().await;

  let err = c1
    .api_client
    .leave_workspace(&workspace_id_c1)
    .await
    .unwrap_err();

  // owner of workspace cannot leave the workspace
  assert_eq!(err.code, ErrorCode::NotEnoughPermissions);
}

#[tokio::test]
async fn add_workspace_member_and_then_member_get_member_list() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let member = TestClient::new_user_without_ws_conn().await;
  let guest = TestClient::new_user_without_ws_conn().await;

  let workspace_id = owner.workspace_id().await;
  owner
    .invite_and_accepted_workspace_member(&workspace_id, &member, AFRole::Member)
    .await
    .unwrap();
  owner
    .invite_and_accepted_workspace_member(&workspace_id, &guest, AFRole::Guest)
    .await
    .unwrap();

  // member should be able to get the member list of the workspace, guest should be excluded
  let members = member.get_workspace_members(&workspace_id).await;
  assert_eq!(members.len(), 2);

  // guest should not be able to get the member list of the workspace, only their own info
  // and the owner
  let members = guest
    .try_get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert_eq!(members.len(), 2);
}

#[tokio::test]
async fn workspace_member_through_user_id() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let member_1 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = owner.workspace_id().await;

  let owner_member = owner
    .get_workspace_member(workspace_id, owner.uid().await)
    .await;
  assert_eq!(owner_member.role, AFRole::Owner);

  owner
    .invite_and_accepted_workspace_member(&workspace_id, &member_1, AFRole::Member)
    .await
    .unwrap();

  let member_1_member = member_1
    .get_workspace_member(workspace_id, member_1.uid().await)
    .await;
  assert_eq!(member_1_member.role, AFRole::Member);

  assert_ne!(owner_member.role, member_1_member.role);
}
