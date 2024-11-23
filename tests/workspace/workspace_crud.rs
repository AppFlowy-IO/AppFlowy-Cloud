use client_api_test::generate_unique_registered_user_client;
use collab_entity::CollabType;
use database_entity::dto::QueryCollabParams;
use shared_entity::dto::workspace_dto::CreateWorkspaceParam;
use shared_entity::dto::workspace_dto::PatchWorkspaceParam;

#[tokio::test]
async fn workspace_list_database() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = c.get_workspaces().await.unwrap()[0]
    .workspace_id
    .to_string();

  {
    let dbs = c.list_databases(&workspace_id).await.unwrap();
    assert_eq!(dbs.len(), 1, "{:?}", dbs);
    let todos_db = &dbs[0];
    assert_eq!(todos_db.views.len(), 1);
    assert_eq!(todos_db.views[0].name, "To-dos");
    {
      let db_row_ids = c
        .list_database_row_ids(&workspace_id, &todos_db.id)
        .await
        .unwrap();
      assert_eq!(db_row_ids.len(), 5, "{:?}", db_row_ids);
    }

    {
      let db_row_ids = c
        .list_database_row_ids(&workspace_id, &todos_db.id)
        .await
        .unwrap();
      assert_eq!(db_row_ids.len(), 5, "{:?}", db_row_ids);
      {
        let db_row_ids: Vec<&str> = db_row_ids.iter().map(|s| s.id.as_str()).collect();
        let db_row_ids: &[&str] = &db_row_ids;
        let db_row_details = c
          .list_database_row_details(&workspace_id, &todos_db.id, db_row_ids)
          .await
          .unwrap();
        assert_eq!(db_row_details.len(), 5, "{:#?}", db_row_details);

        // cells: {
        //     "Multiselect": {
        //         "field_type": "MultiSelect",
        //         "last_modified": "2024-08-16T07:23:57+00:00",
        //         "created_at": "2024-08-16T07:23:35+00:00",
        //         "data": "looks great,fast",
        //     },
        //     "Description": {
        //         "field_type": "RichText",
        //         "last_modified": "2024-08-16T07:17:03+00:00",
        //         "created_at": "2024-08-16T07:16:51+00:00",
        //         "data": "Install AppFlowy Mobile",
        //     },
        //     "Status": {
        //         "data": "To Do",
        //         "field_type": "SingleSelect",
        //     },
        // },
        let _ = db_row_details
          .into_iter()
          .find(|row| {
            row.cells["Multiselect"]["field_type"] == "MultiSelect"
              && row.cells["Multiselect"]["last_modified"] == "2024-08-16T07:23:57+00:00"
              && row.cells["Multiselect"]["created_at"] == "2024-08-16T07:23:35+00:00"
              && row.cells["Multiselect"]["data"] == "looks great,fast"
              // Description
              && row.cells["Description"]["field_type"] == "RichText"
              && row.cells["Description"]["last_modified"] == "2024-08-16T07:17:03+00:00"
              && row.cells["Description"]["created_at"] == "2024-08-16T07:16:51+00:00"
              && row.cells["Description"]["data"] == "Install AppFlowy Mobile"
              // Status
              && row.cells["Status"]["data"] == "To Do"
              && row.cells["Status"]["field_type"] == "SingleSelect"
          })
          .unwrap();
      }
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
