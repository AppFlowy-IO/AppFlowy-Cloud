use std::collections::HashMap;

use client_api_test::generate_unique_registered_user_client;
use collab_entity::CollabType;
use database_entity::dto::QueryCollabParams;
use serde_json::json;
use shared_entity::dto::workspace_dto::AFDatabaseField;
use shared_entity::dto::workspace_dto::CreateWorkspaceParam;
use shared_entity::dto::workspace_dto::PatchWorkspaceParam;

#[tokio::test]
async fn workspace_list_database() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = c.get_workspaces().await.unwrap()[0].workspace_id;

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
      let mut db_fields = c
        .get_database_fields(&workspace_id, &todos_db.id)
        .await
        .unwrap();

      // convert to hashset to check for equeality
      db_fields.sort_by(|a, b| a.id.cmp(&b.id));
      let mut expected = vec![
        AFDatabaseField {
          id: "wdX8DG".to_string(),
          name: "Multiselect".to_string(),
          field_type: "MultiSelect".to_string(),
          type_option: {
            let mut options = HashMap::new();
            options.insert(
              "content".to_string(),
              json!({
                "disable_color": false,
                "options": [
                  {"color": "Purple", "id": "4PDn", "name": "get things done"},
                  {"color": "Blue", "id": "Bpyg", "name": "self-host"},
                  {"color": "Aqua", "id": "GOQj", "name": "open source"},
                  {"color": "Green", "id": "BD-T", "name": "looks great"},
                  {"color": "Lime", "id": "6UxM", "name": "fast"},
                  {"color": "Yellow", "id": "g2Uq", "name": "Claude 3"},
                  {"color": "Orange", "id": "Tt-J", "name": "GPT-4o"},
                  {"color": "LightPink", "id": "5QDY", "name": "Q&A"},
                  {"color": "Pink", "id": "XYUx", "name": "news"},
                  {"color": "Purple", "id": "hoZx", "name": "social"},
                ],
              }),
            );
            options
          },
          is_primary: false,
        },
        AFDatabaseField {
          id: "SqwRg1".to_string(),
          name: "Status".to_string(),
          field_type: "SingleSelect".to_string(),
          type_option: {
            let mut options = HashMap::new();
            options.insert(
              "content".to_string(),
              json!({
                "disable_color": false,
                "options": [
                  {"color": "Purple", "id": "CEZD", "name": "To Do"},
                  {"color": "Orange", "id": "TznH", "name": "Doing"},
                  {"color": "Yellow", "id": "__n6", "name": "✅ Done"},
                ],
              }),
            );
            options
          },
          is_primary: false,
        },
        AFDatabaseField {
          id: "phVRgL".to_string(),
          name: "Description".to_string(),
          field_type: "RichText".to_string(),
          type_option: {
            let mut options = HashMap::new();
            options.insert("data".to_string(), json!(""));
            options
          },
          is_primary: true,
        },
        AFDatabaseField {
          id: "KinVda".to_string(),
          name: "Tasks".to_string(),
          field_type: "Checklist".to_string(),
          type_option: HashMap::new(),
          is_primary: false,
        },
        AFDatabaseField {
          id: "3AE6iK".to_string(),
          name: "Last modified".to_string(),
          field_type: "LastEditedTime".to_string(),
          type_option: {
            let mut options = HashMap::new();
            options.insert("date_format".to_string(), json!(3));
            options.insert("field_type".to_string(), json!(8));
            options.insert("include_time".to_string(), json!(true));
            options.insert("time_format".to_string(), json!(1));
            options
          },
          is_primary: false,
        },
      ];
      expected.sort_by(|a, b| a.id.cmp(&b.id));
      assert_eq!(db_fields, expected, "{:#?}", db_fields);
    }
    {
      let db_row_ids = c
        .list_database_row_ids_updated(&workspace_id, &todos_db.id, None)
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
      workspace_icon: Some("🏡".to_string()),
    })
    .await
    .unwrap();
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 2);

  let _ = workspaces
    .iter()
    .find(|w| {
      w.workspace_name == "my_workspace"
        && w.icon == "🏡"
        && w.workspace_id == newly_added_workspace.workspace_id
    })
    .unwrap();

  // Workspace need to have at least one collab
  let workspace_id = newly_added_workspace.workspace_id;
  let _ = c
    .get_collab(QueryCollabParams::new(
      workspace_id,
      CollabType::Folder,
      workspace_id,
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
