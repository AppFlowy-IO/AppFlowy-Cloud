use client_api_test_util::generate_unique_registered_user_client;
use shared_entity::dto::workspace_dto::CreateWorkspaceParam;

#[tokio::test]
async fn add_and_delete_workspace_for_user() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.0.len(), 1);
  let newly_addad_workspace = c
    .create_workspace(CreateWorkspaceParam {
      workspace_name: Some("my_workspace".to_string()),
    })
    .await
    .unwrap();
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.0.len(), 2);

  let _ = workspaces
    .0
    .iter()
    .find(|w| {
      w.workspace_name == "my_workspace" && w.workspace_id == newly_addad_workspace.workspace_id
    })
    .unwrap();

  c.delete_workspace(&newly_addad_workspace.workspace_id.to_string())
    .await
    .unwrap();
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.0.len(), 1);
}
