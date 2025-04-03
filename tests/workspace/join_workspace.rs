use client_api::entity::WorkspaceInviteCodeParams;
use client_api_test::generate_unique_registered_user_client;

#[tokio::test]
async fn join_workspace_by_invite_code() {
  let (owner_client, _) = generate_unique_registered_user_client().await;
  let workspaces = owner_client.get_workspaces().await.unwrap();
  let workspace_id = workspaces[0].workspace_id;
  let (invitee_client, _) = generate_unique_registered_user_client().await;
  let invitation_code = owner_client
    .create_workspace_invitation_code(
      &workspace_id,
      &WorkspaceInviteCodeParams {
        validity_period_hours: None,
      },
    )
    .await
    .unwrap()
    .code;
  let invited_workspace_id = invitee_client
    .join_workspace_by_invitation_code(&invitation_code)
    .await
    .unwrap()
    .workspace_id;
  assert_eq!(workspace_id, invited_workspace_id);
  assert!(invitee_client
    .get_workspaces()
    .await
    .unwrap()
    .iter()
    .any(|w| w.workspace_id == invited_workspace_id));
}
