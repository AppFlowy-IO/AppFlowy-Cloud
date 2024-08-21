use client_api_test::generate_unique_registered_user_client;

#[tokio::test]
async fn get_workpace_folder() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id.to_string();

  let folder_view = c.get_workspace_folder(&workspace_id, None).await.unwrap();
  assert_eq!(folder_view.name, "Workspace");
  assert_eq!(folder_view.children[0].name, "Getting started");
  assert_eq!(folder_view.children[0].children.len(), 0);
}
