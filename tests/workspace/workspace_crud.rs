use client_api_test::generate_unique_registered_user_client;
use collab_entity::CollabType;
use database_entity::dto::QueryCollabParams;
use shared_entity::dto::workspace_dto::CreateWorkspaceParam;
use shared_entity::dto::workspace_dto::PatchWorkspaceParam;

#[tokio::test]
async fn add_and_delete_workspace_for_user() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.0.len(), 1);
  let newly_added_workspace = c
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
      w.workspace_name == "my_workspace" && w.workspace_id == newly_added_workspace.workspace_id
    })
    .unwrap();

  // Workspace need to have at least one collab
  let workspace_id = newly_added_workspace.workspace_id.to_string();
  let _ = c
    .get_collab(QueryCollabParams::new(
      &workspace_id,
      CollabType::Folder,
      &workspace_id,
    ))
    .await
    .unwrap();

  c.delete_workspace(&workspace_id).await.unwrap();
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.0.len(), 1);
}

#[tokio::test]
async fn test_workspace_rename_and_icon_change() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = c
    .get_workspaces()
    .await
    .unwrap()
    .0
    .first()
    .unwrap()
    .workspace_id;
  let desired_new_name = "tom's workspace";

  {
    c.patch_workspace(PatchWorkspaceParam {
      workspace_id,
      workspace_name: Some(desired_new_name.to_string()),
      ..Default::default()
    })
    .await
    .expect("Failed to rename workspace");

    let workspaces = c.get_workspaces().await.expect("Failed to get workspaces");
    let actual_new_name = &workspaces
      .0
      .first()
      .expect("No workspace found")
      .workspace_name;
    assert_eq!(actual_new_name, desired_new_name);
  }

  {
    c.patch_workspace(PatchWorkspaceParam {
      workspace_id,
      workspace_name: None,
      ..Default::default()
    })
    .await
    .expect("Failed to rename workspace");
    let workspaces = c.get_workspaces().await.expect("Failed to get workspaces");
    let actual_new_name = &workspaces
      .0
      .first()
      .expect("No workspace found")
      .workspace_name;
    assert_eq!(actual_new_name, desired_new_name);
  }
  {
    c.patch_workspace(PatchWorkspaceParam {
      workspace_id,
      workspace_icon: Some("icon123".to_string()),
      ..Default::default()
    })
    .await
    .expect("Failed to change icon");
    let workspaces = c.get_workspaces().await.expect("Failed to get workspaces");
    let icon = &workspaces.0.first().expect("No workspace found").icon;
    assert_eq!(icon, "icon123");
  }
  {
    c.patch_workspace(PatchWorkspaceParam {
      workspace_id,
      workspace_name: Some("new_name456".to_string()),
      workspace_icon: Some("new_icon456".to_string()),
    })
    .await
    .expect("Failed to change icon");
    let workspaces = c.get_workspaces().await.expect("Failed to get workspaces");
    let workspace = workspaces.0.first().expect("No workspace found");

    let icon = workspace.icon.as_str();
    let name = workspace.workspace_name.as_str();
    assert_eq!(icon, "new_icon456");
    assert_eq!(name, "new_name456");
  }
}
