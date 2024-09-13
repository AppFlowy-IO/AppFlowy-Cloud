use app_error::ErrorCode;
use client_api_test::generate_unique_registered_user_client;
use database_entity::dto::{AFRole, AFWorkspaceInvitationStatus};
use shared_entity::dto::workspace_dto::{QueryWorkspaceParam, WorkspaceMemberInvitation};

#[tokio::test]
async fn invite_workspace_crud() {
  let (alice_client, alice) = generate_unique_registered_user_client().await;
  let alice_workspace_id = alice_client
    .get_workspaces()
    .await
    .unwrap()
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
  let invite_id = pending_invs.first().unwrap().invite_id.to_string();

  // get invitation by id
  let invitation = bob_client
    .get_workspace_invitation(&invite_id)
    .await
    .unwrap();

  assert_eq!(invitation.inviter_email, Some(alice.email));
  assert_eq!(invitation.status, AFWorkspaceInvitationStatus::Pending);
  assert_eq!(invitation.member_count.unwrap_or(0), 1);

  let (charlie_client, _charlie) = generate_unique_registered_user_client().await;
  let err = charlie_client
    .get_workspace_invitation(&invite_id)
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::NotInviteeOfWorkspaceInvitation);
  let err = charlie_client
    .accept_workspace_invitation(&invite_id)
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::NotInviteeOfWorkspaceInvitation);

  bob_client
    .accept_workspace_invitation(&invite_id)
    .await
    .unwrap();

  let invitation = bob_client
    .get_workspace_invitation(&invite_id)
    .await
    .unwrap();

  assert_eq!(invitation.status, AFWorkspaceInvitationStatus::Accepted);
  assert_eq!(invitation.member_count.unwrap_or(0), 2);

  // list invitation with accepted filter
  let accepted_invs = bob_client
    .list_workspace_invitations(Some(AFWorkspaceInvitationStatus::Accepted))
    .await
    .unwrap();
  assert_eq!(accepted_invs.len(), 1);

  // workspace now have 2 members
  let member_count = alice_client
    .get_workspaces_opt(QueryWorkspaceParam {
      include_member_count: Some(true),
    })
    .await
    .unwrap()
    .first()
    .unwrap()
    .member_count
    .unwrap();
  assert_eq!(member_count, 2);
}
