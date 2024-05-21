use client_api_test::generate_unique_registered_user_client;
use database_entity::dto::{AFRole, AFWorkspaceInvitationStatus};
use shared_entity::dto::workspace_dto::WorkspaceMemberInvitation;

#[tokio::test]
async fn invite_workspace_crud() {
  let (alice_client, _alice) = generate_unique_registered_user_client().await;
  let alice_workspace_id = alice_client
    .get_workspaces()
    .await
    .unwrap()
    .0
    .first()
    .unwrap()
    .workspace_id;

  let (bob_client, bob) = generate_unique_registered_user_client().await;
  alice_client
    .invite_workspace_members(
      alice_workspace_id.to_string().as_str(),
      vec![WorkspaceMemberInvitation {
        email: bob.email.clone(),
        role: AFRole::Member,
      }],
    )
    .await
    .unwrap();

  // list invitation with no filter
  let invitations_for_bob = bob_client.list_workspace_invitations(None).await.unwrap();
  assert_eq!(invitations_for_bob.len(), 1);

  // list invitation with accepted filter
  let accepted_invs = bob_client
    .list_workspace_invitations(Some(AFWorkspaceInvitationStatus::Accepted))
    .await
    .unwrap();
  assert_eq!(accepted_invs.len(), 0);

  // list invitation with rejected filter
  let rejected_invs = bob_client
    .list_workspace_invitations(Some(AFWorkspaceInvitationStatus::Rejected))
    .await
    .unwrap();
  assert_eq!(rejected_invs.len(), 0);

  // list invitation with pending filter
  let pending_invs = bob_client
    .list_workspace_invitations(Some(AFWorkspaceInvitationStatus::Pending))
    .await
    .unwrap();
  assert_eq!(pending_invs.len(), 1);

  // accept invitation
  let target_invite = pending_invs
    .iter()
    .find(|i| i.workspace_id == alice_workspace_id)
    .unwrap();
  bob_client
    .accept_workspace_invitation(target_invite.invite_id.to_string().as_str())
    .await
    .unwrap();

  // list invitation with accepted filter
  let accepted_invs = bob_client
    .list_workspace_invitations(Some(AFWorkspaceInvitationStatus::Accepted))
    .await
    .unwrap();
  assert_eq!(accepted_invs.len(), 1);
}
