use std::{collections::HashSet, time::Duration};

use client_api::entity::{QueryCollab, QueryCollabParams};
use client_api_test::{
  generate_unique_registered_user, generate_unique_registered_user_client, TestClient,
};
use collab::core::origin::CollabClient;
use collab_entity::CollabType;
use collab_folder::{CollabOrigin, Folder};
use serde_json::{json, Value};
use shared_entity::dto::workspace_dto::{
  AddRecentPagesParams, AppendBlockToPageParams, CreateFolderViewParams,
  CreatePageDatabaseViewParams, CreatePageParams, CreateSpaceParams, DuplicatePageParams,
  FavoritePageParams, IconType, MovePageParams, PublishPageParams, SpacePermission,
  UpdatePageExtraParams, UpdatePageIconParams, UpdatePageNameParams, UpdatePageParams,
  UpdateSpaceParams, ViewIcon, ViewLayout,
};
use tokio::time::sleep;
use uuid::Uuid;

async fn get_latest_folder(test_client: &TestClient, workspace_id: &Uuid) -> Folder {
  // Wait for websocket updates
  sleep(Duration::from_secs(1)).await;
  let lock = test_client
    .collabs
    .get(workspace_id)
    .unwrap()
    .collab
    .read()
    .await;
  let collab_type = CollabType::Folder;
  let encoded_collab = lock
    .encode_collab_v1(|collab| collab_type.validate_require_data(collab))
    .unwrap();
  let uid = test_client.uid().await;
  Folder::from_collab_doc_state(
    CollabOrigin::Client(CollabClient::new(uid, test_client.device_id.clone())),
    encoded_collab.into(),
    &workspace_id.to_string(),
    test_client.client_id(workspace_id).await,
  )
  .unwrap()
}

#[tokio::test]
async fn get_page_view() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id;
  let folder_view = c
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let todo = general_space
    .children
    .iter()
    .find(|v| v.name == "To-dos")
    .unwrap();
  let todo_list_view_id = &todo.view_id;
  let resp = c
    .get_workspace_page_view(workspace_id, todo_list_view_id)
    .await
    .unwrap();
  assert_eq!(resp.data.row_data.len(), 5);
  let getting_started = general_space
    .children
    .iter()
    .find(|v| v.name == "Getting started")
    .unwrap();
  let getting_started_view_id = &getting_started.view_id;
  let resp = c
    .get_workspace_page_view(workspace_id, getting_started_view_id)
    .await
    .unwrap();
  assert_eq!(resp.data.row_data.len(), 0);
}

#[tokio::test]
async fn create_new_page_with_database() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id;
  let folder_view = c
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let calendar_page = c
    .create_workspace_page_view(
      workspace_id,
      &CreatePageParams {
        parent_view_id: general_space.view_id,
        layout: ViewLayout::Calendar,
        name: Some("New calendar".to_string()),
        page_data: None,
        view_id: None,
        collab_id: None,
      },
    )
    .await
    .unwrap();
  let grid_page = c
    .create_workspace_page_view(
      workspace_id,
      &CreatePageParams {
        parent_view_id: general_space.view_id,
        layout: ViewLayout::Grid,
        name: Some("New grid".to_string()),
        page_data: None,
        view_id: None,
        collab_id: None,
      },
    )
    .await
    .unwrap();
  let board_page = c
    .create_workspace_page_view(
      workspace_id,
      &CreatePageParams {
        parent_view_id: general_space.view_id,
        layout: ViewLayout::Grid,
        name: Some("New board".to_string()),
        page_data: None,
        view_id: None,
        collab_id: None,
      },
    )
    .await
    .unwrap();
  sleep(Duration::from_secs(1)).await;
  let folder_view = c
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let views_under_general_space: HashSet<_> =
    general_space.children.iter().map(|v| v.view_id).collect();
  for view_id in &[calendar_page.view_id, grid_page.view_id, board_page.view_id] {
    assert!(views_under_general_space.contains(view_id));
    c.get_workspace_page_view(workspace_id, view_id)
      .await
      .unwrap();
  }
}

#[tokio::test]
async fn create_new_folder_view() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id;
  let folder_view = c
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let page = c
    .create_folder_view(
      workspace_id,
      &CreateFolderViewParams {
        parent_view_id: general_space.view_id,
        layout: ViewLayout::Document,
        name: Some("New document".to_string()),
        view_id: None,
        database_id: None,
      },
    )
    .await
    .unwrap();
  sleep(Duration::from_secs(1)).await;
  let folder_view = c
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let view = general_space
    .children
    .iter()
    .find(|v| v.view_id == page.view_id)
    .unwrap();
  assert_eq!(view.name, "New document");
}

#[tokio::test]
async fn create_new_document_page() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id;
  let folder_view = c
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let page = c
    .create_workspace_page_view(
      workspace_id,
      &CreatePageParams {
        parent_view_id: general_space.view_id,
        layout: ViewLayout::Document,
        name: Some("New document".to_string()),
        page_data: None,
        view_id: None,
        collab_id: None,
      },
    )
    .await
    .unwrap();
  let page_with_initial_data = c
    .create_workspace_page_view(
      workspace_id,
      &CreatePageParams {
        parent_view_id: general_space.view_id,
        layout: ViewLayout::Document,
        name: Some("Message extracted from why is the sky blue".to_string()),
        page_data: Some(json!({
          "type": "page",
          "children": [
            {
              "type": "paragraph",
              "data": {
                "delta": [
                  {
                    "insert": "The sky appears blue due to a phenomenon called Rayleigh scattering."
                  }
                ]
              }
            },
          ]
        })),
        view_id: None,
        collab_id: None,
      },
    )
    .await
    .unwrap();
  sleep(Duration::from_secs(1)).await;
  let folder_view = c
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let view = general_space
    .children
    .iter()
    .find(|v| v.view_id == page.view_id)
    .unwrap();
  assert_eq!(view.name, "New document");
  c.get_collab(QueryCollabParams {
    workspace_id,
    inner: QueryCollab {
      object_id: page.view_id,
      collab_type: CollabType::Document,
    },
  })
  .await
  .unwrap();
  general_space
    .children
    .iter()
    .find(|v| v.view_id == page_with_initial_data.view_id)
    .unwrap();
  c.get_collab(QueryCollabParams {
    workspace_id,
    inner: QueryCollab {
      object_id: page_with_initial_data.view_id,
      collab_type: CollabType::Document,
    },
  })
  .await
  .unwrap();
}

#[tokio::test]
async fn append_block_to_page() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id;
  let folder_view = c
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let getting_started = general_space
    .children
    .iter()
    .find(|v| v.name == "Getting started")
    .unwrap();
  let getting_started_view_id = &getting_started.view_id;
  c.append_block_to_page(
    workspace_id,
    getting_started_view_id,
    &AppendBlockToPageParams {
      blocks: vec![json!({
        "type": "paragraph",
        "data": {
          "delta": [
            {
              "insert": "The sky appears blue due to a phenomenon called Rayleigh scattering."
            }
          ]
        }
      })],
    },
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn create_new_chat_page() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id;
  let folder_view = c
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let page = c
    .create_workspace_page_view(
      workspace_id,
      &CreatePageParams {
        parent_view_id: general_space.view_id,
        layout: ViewLayout::Chat,
        name: Some("New chat".to_string()),
        page_data: None,
        view_id: None,
        collab_id: None,
      },
    )
    .await
    .unwrap();
  sleep(Duration::from_secs(1)).await;
  let folder_view = c
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  general_space
    .children
    .iter()
    .find(|v| v.view_id == page.view_id)
    .unwrap();
  assert_eq!(
    c.get_chat_settings(&workspace_id, &page.view_id.to_string())
      .await
      .unwrap()
      .name,
    "New chat"
  );
}

#[tokio::test]
async fn move_page_to_another_space() {
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let workspace_id = app_client.workspace_id().await;
  app_client.open_workspace_collab(workspace_id).await;
  app_client
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = folder_view
    .children
    .iter()
    .find(|v| v.name == "General")
    .unwrap()
    .clone();
  let todo_view_id = general_space
    .children
    .iter()
    .find(|v| v.name == "To-dos")
    .map(|v| v.view_id)
    .unwrap();
  let shared_space = &folder_view
    .children
    .iter()
    .find(|v| v.name == "Shared")
    .unwrap()
    .clone();
  web_client
    .api_client
    .move_workspace_page_view(
      workspace_id,
      &todo_view_id,
      &MovePageParams {
        new_parent_view_id: shared_space.view_id.to_string(),
        prev_view_id: None,
      },
    )
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let first_children_id = folder
    .get_view(&shared_space.view_id.to_string(), web_client.uid().await)
    .unwrap()
    .children[0]
    .id
    .parse::<Uuid>()
    .unwrap();
  assert_eq!(first_children_id, todo_view_id);
}

#[tokio::test]
async fn move_page_to_trash_then_restore() {
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let uid = web_client.uid().await;
  let workspace_id = app_client.workspace_id().await;
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let view_ids_to_be_deleted = [
    general_space.children[0].view_id,
    general_space.children[1].view_id,
  ];
  app_client.open_workspace_collab(workspace_id).await;
  app_client
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();
  for view_id in view_ids_to_be_deleted.iter() {
    web_client
      .api_client
      .move_workspace_page_view_to_trash(workspace_id, view_id)
      .await
      .unwrap();
  }
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let views_in_trash_for_app = folder
    .get_my_trash_sections(uid)
    .iter()
    .flat_map(|v| Uuid::parse_str(&v.id).ok())
    .collect::<HashSet<_>>();
  for view_id in view_ids_to_be_deleted.iter() {
    assert!(views_in_trash_for_app.contains(view_id));
  }
  let views_in_trash_for_web = web_client
    .api_client
    .get_workspace_trash(&workspace_id)
    .await
    .unwrap()
    .views
    .iter()
    .map(|v| v.view.view_id)
    .collect::<HashSet<_>>();
  for view_id in view_ids_to_be_deleted.iter() {
    assert!(views_in_trash_for_web.contains(view_id));
  }

  web_client
    .api_client
    .restore_workspace_page_view_from_trash(workspace_id, &view_ids_to_be_deleted[0])
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  assert!(!folder
    .get_my_trash_sections(uid)
    .iter()
    .any(|v| v.id == view_ids_to_be_deleted[0].to_string()));
  let view_found = web_client
    .api_client
    .get_workspace_trash(&workspace_id)
    .await
    .unwrap()
    .views
    .iter()
    .any(|v| v.view.view_id == view_ids_to_be_deleted[0]);
  assert!(!view_found);
  web_client
    .api_client
    .restore_all_workspace_page_views_from_trash(workspace_id)
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  assert!(!folder
    .get_my_trash_sections(uid)
    .iter()
    .any(|v| v.id == view_ids_to_be_deleted[1].to_string()));
  let view_found = web_client
    .api_client
    .get_workspace_trash(&workspace_id)
    .await
    .unwrap()
    .views
    .iter()
    .any(|v| v.view.view_id == view_ids_to_be_deleted[1]);
  assert!(!view_found);
}

#[tokio::test]
async fn move_page_with_child_to_trash_then_restore() {
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let uid = web_client.uid().await;
  let workspace_id = app_client.workspace_id().await;
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  app_client.open_workspace_collab(workspace_id).await;
  app_client
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();
  web_client
    .api_client
    .move_workspace_page_view_to_trash(workspace_id, &general_space.view_id)
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let views_in_trash_for_app = folder
    .get_my_trash_sections(uid)
    .iter()
    .flat_map(|v| Uuid::parse_str(&v.id).ok())
    .collect::<HashSet<_>>();
  assert!(views_in_trash_for_app.contains(&general_space.view_id));
  for view in general_space.children.iter() {
    assert!(!views_in_trash_for_app.contains(&view.view_id));
  }
  let views_in_trash_for_web = web_client
    .api_client
    .get_workspace_trash(&workspace_id)
    .await
    .unwrap()
    .views
    .iter()
    .map(|v| v.view.view_id)
    .collect::<HashSet<_>>();
  assert!(views_in_trash_for_web.contains(&general_space.view_id));

  web_client
    .api_client
    .restore_workspace_page_view_from_trash(workspace_id, &general_space.view_id)
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  assert!(!folder
    .get_my_trash_sections(uid)
    .iter()
    .any(|v| v.id == general_space.view_id.to_string()));
  let view_found = web_client
    .api_client
    .get_workspace_trash(&workspace_id)
    .await
    .unwrap()
    .views
    .iter()
    .any(|v| v.view.view_id == general_space.view_id);
  assert!(!view_found);
}

#[tokio::test]
async fn move_page_with_child_to_trash_then_delete_permanently() {
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let uid = web_client.uid().await;
  let workspace_id = app_client.workspace_id().await;
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  app_client.open_workspace_collab(workspace_id).await;
  app_client
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();
  web_client
    .api_client
    .move_workspace_page_view_to_trash(workspace_id, &general_space.view_id)
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let views_in_trash_for_app = folder
    .get_my_trash_sections(uid)
    .iter()
    .flat_map(|v| Uuid::parse_str(&v.id).ok())
    .collect::<HashSet<_>>();
  assert!(views_in_trash_for_app.contains(&general_space.view_id));
  for view in general_space.children.iter() {
    assert!(!views_in_trash_for_app.contains(&view.view_id));
  }
  let views_in_trash_for_web = web_client
    .api_client
    .get_workspace_trash(&workspace_id)
    .await
    .unwrap()
    .views
    .iter()
    .map(|v| v.view.view_id)
    .collect::<HashSet<_>>();
  assert!(views_in_trash_for_web.contains(&general_space.view_id));

  web_client
    .api_client
    .delete_workspace_page_view_from_trash(workspace_id, &general_space.view_id)
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  assert!(folder
    .get_view(&general_space.view_id.to_string(), uid)
    .is_none());
  assert!(!folder
    .get_my_trash_sections(uid)
    .iter()
    .any(|v| v.id == general_space.view_id.to_string()));
  let view_found = web_client
    .api_client
    .get_workspace_trash(&workspace_id)
    .await
    .unwrap()
    .views
    .iter()
    .any(|v| v.view.view_id == general_space.view_id);
  assert!(!view_found);
}

#[tokio::test]
async fn move_page_with_child_to_trash_then_delete_all_permanently() {
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let uid = web_client.uid().await;
  let workspace_id = app_client.workspace_id().await;
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  app_client.open_workspace_collab(workspace_id).await;
  app_client
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();
  web_client
    .api_client
    .move_workspace_page_view_to_trash(workspace_id, &general_space.view_id)
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let views_in_trash_for_app = folder
    .get_my_trash_sections(uid)
    .iter()
    .flat_map(|v| Uuid::parse_str(&v.id).ok())
    .collect::<HashSet<_>>();
  assert!(views_in_trash_for_app.contains(&general_space.view_id));
  for view in general_space.children.iter() {
    assert!(!views_in_trash_for_app.contains(&view.view_id));
  }
  let views_in_trash_for_web = web_client
    .api_client
    .get_workspace_trash(&workspace_id)
    .await
    .unwrap()
    .views
    .iter()
    .map(|v| v.view.view_id)
    .collect::<HashSet<_>>();
  assert!(views_in_trash_for_web.contains(&general_space.view_id));

  web_client
    .api_client
    .delete_all_workspace_page_views_from_trash(workspace_id)
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  assert!(folder
    .get_view(&general_space.view_id.to_string(), uid)
    .is_none());
  assert!(!folder
    .get_my_trash_sections(uid)
    .iter()
    .any(|v| v.id == general_space.view_id.to_string()));
  let view_found = web_client
    .api_client
    .get_workspace_trash(&workspace_id)
    .await
    .unwrap()
    .views
    .iter()
    .any(|v| v.view.view_id == general_space.view_id);
  assert!(!view_found);
}

#[tokio::test]
async fn update_page() {
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let uid = web_client.uid().await;
  let workspace_id = app_client.workspace_id().await;
  app_client.open_workspace_collab(workspace_id).await;
  app_client
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let view_id_to_be_updated = general_space.children[0].view_id;
  web_client
    .api_client
    .update_workspace_page_view(
      workspace_id,
      &view_id_to_be_updated,
      &UpdatePageParams {
        name: "New Name".to_string(),
        icon: Some(ViewIcon {
          ty: IconType::Emoji,
          value: "🚀".to_string(),
        }),
        is_locked: None,
        extra: Some(json!({"is_pinned": true})),
      },
    )
    .await
    .unwrap();

  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let updated_view = folder
    .get_view(&view_id_to_be_updated.to_string(), uid)
    .unwrap();
  assert_eq!(updated_view.name, "New Name");
  assert_eq!(
    updated_view.icon,
    Some(collab_folder::ViewIcon {
      ty: collab_folder::IconType::Emoji,
      value: "🚀".to_string(),
    })
  );
  assert_eq!(
    updated_view.extra,
    Some(json!({"is_pinned": true}).to_string())
  );
  assert_eq!(updated_view.is_locked, None);
  web_client
    .api_client
    .update_page_name(
      workspace_id,
      &view_id_to_be_updated,
      &UpdatePageNameParams {
        name: "Another Name".to_string(),
      },
    )
    .await
    .unwrap();
  web_client
    .api_client
    .update_page_icon(
      workspace_id,
      &view_id_to_be_updated,
      &UpdatePageIconParams {
        icon: ViewIcon {
          ty: IconType::Emoji,
          value: "😎".to_string(),
        },
      },
    )
    .await
    .unwrap();
  web_client
    .api_client
    .update_page_extra(
      workspace_id,
      &view_id_to_be_updated,
      &UpdatePageExtraParams {
        extra: json!({"is_pinned": false}).to_string(),
      },
    )
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let updated_view = folder
    .get_view(&view_id_to_be_updated.to_string(), uid)
    .unwrap();
  assert_eq!(updated_view.name, "Another Name");
  assert_eq!(
    updated_view.icon,
    Some(collab_folder::ViewIcon {
      ty: collab_folder::IconType::Emoji,
      value: "😎".to_string(),
    })
  );
  assert_eq!(
    updated_view.extra,
    Some(json!({"is_pinned": false}).to_string())
  );
  web_client
    .api_client
    .remove_page_icon(workspace_id, &view_id_to_be_updated)
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let updated_view = folder
    .get_view(&view_id_to_be_updated.to_string(), uid)
    .unwrap();
  assert_eq!(updated_view.icon, None);
}

#[tokio::test]
async fn favorite_page() {
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let uid = web_client.uid().await;
  let workspace_id = app_client.workspace_id().await;
  app_client.open_workspace_collab(workspace_id).await;
  app_client
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let favorite_view_id = general_space.children[0].view_id;
  web_client
    .api_client
    .favorite_page_view(
      workspace_id,
      &favorite_view_id,
      &FavoritePageParams {
        is_favorite: true,
        is_pinned: true,
      },
    )
    .await
    .unwrap();

  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let favorite_view = folder.get_view(&favorite_view_id.to_string(), uid).unwrap();
  assert!(favorite_view.is_favorite);
}

#[tokio::test]
async fn create_space() {
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let uid = web_client.uid().await;
  let workspace_id = app_client.workspace_id().await;
  app_client.open_workspace_collab(workspace_id).await;
  app_client
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();
  let public_space = web_client
    .api_client
    .create_space(
      workspace_id,
      &CreateSpaceParams {
        space_permission: SpacePermission::PublicToAll,
        name: "Public Space".to_string(),
        space_icon: "space_icon_1".to_string(),
        space_icon_color: "0xFFA34AFD".to_string(),
        view_id: None,
      },
    )
    .await
    .unwrap();
  web_client
    .api_client
    .create_space(
      workspace_id,
      &CreateSpaceParams {
        space_permission: SpacePermission::Private,
        name: "Private Space".to_string(),
        space_icon: "space_icon_2".to_string(),
        space_icon_color: "0xFFA34AFD".to_string(),
        view_id: None,
      },
    )
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let view = folder
    .get_view(&public_space.view_id.to_string(), uid)
    .unwrap();
  let space_info: Value = serde_json::from_str(view.extra.as_ref().unwrap()).unwrap();
  assert!(space_info["is_space"].as_bool().unwrap());
  assert_eq!(
    space_info["space_permission"].as_u64().unwrap() as u8,
    SpacePermission::PublicToAll as u8
  );
  assert_eq!(space_info["space_icon"].as_str().unwrap(), "space_icon_1");
  assert_eq!(
    space_info["space_icon_color"].as_str().unwrap(),
    "0xFFA34AFD"
  );
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), Some(workspace_id))
    .await
    .unwrap();
  folder_view
    .children
    .iter()
    .find(|v| v.name == "Public Space")
    .unwrap();
  let private_space = folder_view
    .children
    .iter()
    .find(|v| v.name == "Private Space")
    .unwrap();
  assert!(private_space.is_private);

  web_client
    .api_client
    .update_space(
      workspace_id,
      &private_space.view_id,
      &UpdateSpaceParams {
        space_permission: SpacePermission::PublicToAll,
        name: "Renamed Space".to_string(),
        space_icon: "space_icon_3".to_string(),
        space_icon_color: "#000000".to_string(),
      },
    )
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let view = folder
    .get_view(&private_space.view_id.to_string(), uid)
    .unwrap();
  let space_info: Value = serde_json::from_str(view.extra.as_ref().unwrap()).unwrap();
  assert!(space_info["is_space"].as_bool().unwrap());
  assert_eq!(
    space_info["space_permission"].as_u64().unwrap() as u8,
    SpacePermission::PublicToAll as u8
  );
  assert_eq!(space_info["space_icon"].as_str().unwrap(), "space_icon_3");
  assert_eq!(space_info["space_icon_color"].as_str().unwrap(), "#000000");
}

#[tokio::test]
async fn publish_page() {
  let registered_user = generate_unique_registered_user().await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let workspace_id = web_client.workspace_id().await;
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let database_page_id = general_space
    .children
    .iter()
    .find(|v| v.name == "To-dos")
    .unwrap()
    .view_id;
  let document_page_id = general_space
    .children
    .iter()
    .find(|v| v.name == "Getting started")
    .unwrap()
    .view_id;
  let page_to_be_published = vec![database_page_id, document_page_id];
  for view_id in &page_to_be_published {
    web_client
      .api_client
      .publish_page(
        workspace_id,
        view_id,
        &PublishPageParams {
          publish_name: None,
          visible_database_view_ids: None,
          comments_enabled: None,
          duplicate_enabled: None,
        },
      )
      .await
      .unwrap();
  }
  let publish_namespace = web_client
    .api_client
    .get_workspace_publish_namespace(&workspace_id)
    .await
    .unwrap();
  let published_view = web_client
    .api_client
    .get_published_outline(&publish_namespace)
    .await
    .unwrap();
  let published_view_ids: HashSet<_> = published_view
    .children
    .iter()
    .find(|v| v.name == "General")
    .unwrap()
    .children
    .iter()
    .flat_map(|v| Uuid::parse_str(&v.view_id))
    .collect();
  for view_id in &page_to_be_published {
    assert!(published_view_ids.contains(view_id));
  }
  for view_id in &page_to_be_published {
    web_client
      .api_client
      .unpublish_page(workspace_id, view_id)
      .await
      .unwrap();
  }
  let published_view = web_client
    .api_client
    .get_published_outline(&publish_namespace)
    .await
    .unwrap();
  assert_eq!(published_view.children.len(), 0);
}

#[tokio::test]
async fn duplicate_view() {
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let uid = web_client.uid().await;
  let workspace_id = app_client.workspace_id().await;
  app_client.open_workspace_collab(workspace_id).await;
  app_client
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  web_client
    .api_client
    .duplicate_view_and_children(
      workspace_id,
      &general_space.view_id,
      &DuplicatePageParams {
        suffix: Some(" (Copy)".to_string()),
      },
    )
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let duplicated_space_id = folder
    .get_view(&workspace_id.to_string(), uid)
    .unwrap()
    .children
    .iter()
    .find(|v| folder.get_view(&v.id, uid).unwrap().name == "General (Copy)")
    .unwrap()
    .id
    .clone();
  let duplicated_views = folder.get_view_recursively(&duplicated_space_id, uid);
  assert_eq!(duplicated_views.len(), 6);
}

#[tokio::test]
async fn create_database_page_view() {
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let uid = web_client.uid().await;
  let workspace_id = app_client.workspace_id().await;
  app_client.open_workspace_collab(workspace_id).await;
  app_client
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let todo_folder_view = general_space
    .children
    .iter()
    .find(|v| v.name == "To-dos")
    .unwrap();
  let todo_list_view_id = todo_folder_view.view_id;
  web_client
    .api_client
    .create_database_view(
      workspace_id,
      &todo_list_view_id,
      &CreatePageDatabaseViewParams {
        layout: ViewLayout::Grid,
        name: Some("Grid View".to_string()),
      },
    )
    .await
    .unwrap();
  let folder = get_latest_folder(&app_client, &workspace_id).await;
  let todo_view = folder
    .get_view(&todo_list_view_id.to_string(), uid)
    .unwrap();
  let grid_view = todo_view
    .children
    .iter()
    .find_map(|v| {
      folder.get_view(&v.id, uid).map(|view| {
        if view.name == "Grid View" {
          Some(view)
        } else {
          None
        }
      })
    })
    .flatten()
    .unwrap();
  let page_collab = web_client
    .api_client
    .get_workspace_page_view(workspace_id, &grid_view.id.parse().unwrap())
    .await
    .unwrap();
  assert_eq!(page_collab.data.row_data.len(), 5);
  assert_eq!(page_collab.view.layout, ViewLayout::Grid);
}

#[tokio::test]
async fn add_recent_pages() {
  let registered_user = generate_unique_registered_user().await;
  let app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let workspace_id = app_client.workspace_id().await;
  let folder_view = app_client
    .api_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let child_view_ids: Vec<_> = general_space.children.iter().map(|v| v.view_id).collect();
  for _ in 0..2 {
    for view_id in &child_view_ids {
      app_client
        .api_client
        .add_recent_pages(
          workspace_id,
          &AddRecentPagesParams {
            recent_view_ids: vec![view_id.to_string()],
          },
        )
        .await
        .unwrap();
    }
  }
  let recent_section_ids = app_client
    .api_client
    .get_workspace_recent(&workspace_id)
    .await
    .unwrap()
    .views
    .iter()
    .map(|v| v.view.view_id)
    .collect::<Vec<_>>();
  assert_eq!(recent_section_ids, child_view_ids)
}
