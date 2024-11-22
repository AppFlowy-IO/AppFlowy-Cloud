use client_api_test::generate_unique_registered_user_client;
use collab_entity::CollabType;
use database_entity::dto::QueryCollabParams;
use shared_entity::dto::workspace_dto::AFDatabaseField;
use shared_entity::dto::workspace_dto::CreateWorkspaceParam;
use shared_entity::dto::workspace_dto::PatchWorkspaceParam;

#[tokio::test]
async fn workspace_list_database() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = c.get_workspaces().await.unwrap()[0]
    .workspace_id
    .to_string();

  {
    let dbs = c.list_databases(&workspace_id, None).await.unwrap();
    assert_eq!(dbs.len(), 1);

    let db = &dbs[0];

    assert_eq!(db.names.len(), 2);
    assert!(db.names.contains(&String::from("Untitled")));
    assert!(db.names.contains(&String::from("Grid")));

    assert!(db.fields.contains(&AFDatabaseField {
      name: "Last modified".to_string(),
      field_type: "LastEditedTime".to_string(),
    }));
    assert!(db.fields.contains(&AFDatabaseField {
      name: "Multiselect".to_string(),
      field_type: "MultiSelect".to_string(),
    }));
    assert!(db.fields.contains(&AFDatabaseField {
      name: "Tasks".to_string(),
      field_type: "Checklist".to_string(),
    }));
    assert!(db.fields.contains(&AFDatabaseField {
      name: "Status".to_string(),
      field_type: "SingleSelect".to_string(),
    }));
    assert!(db.fields.contains(&AFDatabaseField {
      name: "Description".to_string(),
      field_type: "RichText".to_string(),
    }));
  }

  {
    let dbs = c
      .list_databases(&workspace_id, Some(String::from("nomatch")))
      .await
      .unwrap();
    assert_eq!(dbs.len(), 0);
  }
  {
    let dbs = c
      .list_databases(&workspace_id, Some(String::from("ntitle")))
      .await
      .unwrap();
    assert_eq!(dbs.len(), 1);
    {
      let untitled_db = &dbs[0];
      let db_row_ids = c
        .list_database_row_ids(&workspace_id, &untitled_db.id)
        .await
        .unwrap();
      assert_eq!(db_row_ids.len(), 5, "{:?}", db_row_ids);
    }
  }
  {
    let dbs = c
      .list_databases(&workspace_id, Some(String::from("rid")))
      .await
      .unwrap();
    assert_eq!(dbs.len(), 1);
    {
      let grid_db = &dbs[0];
      let db_row_ids = c
        .list_database_row_ids(&workspace_id, &grid_db.id)
        .await
        .unwrap();
      assert_eq!(db_row_ids.len(), 5, "{:?}", db_row_ids);
    }
  }
}

#[tokio::test]
async fn add_and_delete_workspace_for_user() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let newly_added_workspace = c
    .create_workspace(CreateWorkspaceParam {
      workspace_name: Some("my_workspace".to_string()),
    })
    .await
    .unwrap();
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 2);

  let _ = workspaces
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
  assert_eq!(workspaces.len(), 1);
}

#[tokio::test]
async fn test_workspace_rename_and_icon_change() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = c
    .get_workspaces()
    .await
    .unwrap()
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
    let icon = &workspaces.first().expect("No workspace found").icon;
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
    let workspace = workspaces.first().expect("No workspace found");

    let icon = workspace.icon.as_str();
    let name = workspace.workspace_name.as_str();
    assert_eq!(icon, "new_icon456");
    assert_eq!(name, "new_name456");
  }
}
