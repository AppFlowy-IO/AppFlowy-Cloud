use app_error::ErrorCode;
use client_api::Client;
use client_api_test::generate_unique_registered_user_client;
use database_entity::dto::{AFRole, AFWorkspaceInvitationStatus, AFWorkspaceSettings};
use shared_entity::dto::workspace_dto::WorkspaceMemberInvitation;
use uuid::Uuid;

#[tokio::test]
async fn get_and_set_workspace_by_owner() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap().0;
  let workspace_id = workspaces.first().unwrap().workspace_id.to_string();

  let mut settings = c.get_workspace_settings(&workspace_id).await.unwrap();
  assert!(
    !settings.disable_indexing,
    "indexing should be enabled by default"
  );

  settings.disable_indexing = true;
  c.update_workspace_settings(&workspace_id, &settings)
    .await
    .unwrap();

  let settings = c.get_workspace_settings(&workspace_id).await.unwrap();
  assert!(settings.disable_indexing);
}

#[tokio::test]
async fn get_and_set_workspace_by_non_owner() {
  let (alice_client, _alice) = generate_unique_registered_user_client().await;
  let workspaces = alice_client.get_workspaces().await.unwrap().0;
  let alice_workspace_id = workspaces.first().unwrap().workspace_id;

  let (bob_client, bob) = generate_unique_registered_user_client().await;

  invite_user_to_workspace(&alice_workspace_id, &alice_client, &bob_client, &bob.email).await;

  let resp = bob_client
    .get_workspace_settings(&alice_workspace_id.to_string())
    .await;
  assert!(
    resp.is_err(),
    "non-owner should not have access to workspace settings"
  );
  assert_eq!(resp.err().unwrap().code, ErrorCode::UserUnAuthorized);

  let resp = bob_client
    .update_workspace_settings(
      &alice_workspace_id.to_string(),
      &AFWorkspaceSettings {
        disable_indexing: true,
      },
    )
    .await;
  assert!(
    resp.is_err(),
    "non-owner should not be able to edit workspace settings"
  );
  assert_eq!(resp.err().unwrap().code, ErrorCode::UserUnAuthorized);
}

async fn invite_user_to_workspace(
  workspace_id: &Uuid,
  owner: &Client,
  member: &Client,
  member_email: &str,
) {
  owner
    .invite_workspace_members(
      workspace_id.to_string().as_str(),
      vec![WorkspaceMemberInvitation {
        email: member_email.to_string(),
        role: AFRole::Member,
      }],
    )
    .await
    .unwrap();

  // list invitation with pending filter
  let pending_invs = member
    .list_workspace_invitations(Some(AFWorkspaceInvitationStatus::Pending))
    .await
    .unwrap();
  assert_eq!(pending_invs.len(), 1);

  // accept invitation
  let target_invite = pending_invs
    .iter()
    .find(|i| i.workspace_id == *workspace_id)
    .unwrap();

  member
    .accept_workspace_invitation(target_invite.invite_id.to_string().as_str())
    .await
    .unwrap();
}
