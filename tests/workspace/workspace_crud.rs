use client_api_test_util::generate_unique_registered_user_client;

#[tokio::test]
async fn add_and_delete_workspace_for_user() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.0.len(), 1);
  let newly_addad_workspace = c.add_workspace().await.unwrap();
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.0.len(), 2);

  c.delete_workspace(&newly_addad_workspace.workspace_id.to_string())
    .await
    .unwrap();
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.0.len(), 1);
}
