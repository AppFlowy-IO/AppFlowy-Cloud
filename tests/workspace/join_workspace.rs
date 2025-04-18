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
    .code
    .unwrap();
  let retrieved_invite_code = owner_client
    .get_workspace_invitation_code(&workspace_id)
    .await
    .unwrap()
    .code
    .unwrap();
  assert_eq!(invitation_code, retrieved_invite_code);
  let invitation_code_info = invitee_client
    .get_invitation_code_info(&invitation_code)
    .await
    .unwrap();
  assert_eq!(invitation_code_info.is_member, Some(false));
  assert_eq!(invitation_code_info.member_count, 1);
  assert_eq!(
    invitation_code_info.workspace_name,
    workspaces[0].workspace_name
  );
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
  owner_client
    .delete_workspace_invitation_code(&workspace_id)
    .await
    .unwrap();
  assert!(owner_client
    .get_workspace_invitation_code(&workspace_id)
    .await
    .unwrap()
    .code
    .is_none());
}
