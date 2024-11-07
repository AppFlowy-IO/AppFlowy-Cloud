use std::time::Duration;

use client_api::entity::{QueryCollab, QueryCollabParams};
use client_api_test::{
  generate_unique_registered_user, generate_unique_registered_user_client, TestClient,
};
use collab::{core::origin::CollabClient, preclude::Collab};
use collab_entity::CollabType;
use collab_folder::{CollabOrigin, Folder};
use shared_entity::dto::workspace_dto::{CreatePageParams, ViewLayout};
use tokio::time::sleep;
use uuid::Uuid;

#[tokio::test]
async fn get_page_view() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id;
  let folder_view = c
    .get_workspace_folder(&workspace_id.to_string(), Some(2), None)
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
  let todo_list_view_id = Uuid::parse_str(&todo.view_id).unwrap();
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
  let getting_started_view_id = Uuid::parse_str(&getting_started.view_id).unwrap();
  let resp = c
    .get_workspace_page_view(workspace_id, getting_started_view_id)
    .await
    .unwrap();
  assert_eq!(resp.data.row_data.len(), 0);
}

#[tokio::test]
async fn create_new_document_page() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id;
  let folder_view = c
    .get_workspace_folder(&workspace_id.to_string(), Some(2), None)
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
        parent_view_id: general_space.view_id.clone(),
        layout: ViewLayout::Document,
      },
    )
    .await
    .unwrap();
  sleep(Duration::from_secs(1)).await;
  let folder_view = c
    .get_workspace_folder(&workspace_id.to_string(), Some(2), None)
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
  c.get_collab(QueryCollabParams {
    workspace_id: workspace_id.to_string(),
    inner: QueryCollab {
      object_id: page.view_id,
      collab_type: CollabType::Document,
    },
  })
  .await
  .unwrap();
}

#[tokio::test]
async fn move_page_to_trash() {
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let workspace_id = app_client.workspace_id().await;
  app_client.open_workspace_collab(&workspace_id).await;
  app_client
    .wait_object_sync_complete(&workspace_id)
    .await
    .unwrap();
  let folder_view = web_client
    .api_client
    .get_workspace_folder(&workspace_id.to_string(), Some(2), None)
    .await
    .unwrap();
  let general_space = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let view_id_to_be_deleted = general_space.children[0].view_id.clone();
  web_client
    .api_client
    .move_workspace_page_view_to_trash(
      Uuid::parse_str(&workspace_id).unwrap(),
      view_id_to_be_deleted.clone(),
    )
    .await
    .unwrap();

  // Wait for websocket to receive update
  sleep(Duration::from_secs(1)).await;
  let lock = app_client
    .collabs
    .get(&workspace_id)
    .unwrap()
    .collab
    .read()
    .await;
  let collab: &Collab = (*lock).borrow();
  let collab_type = CollabType::Folder;
  let encoded_collab = collab
    .encode_collab_v1(|collab| collab_type.validate_require_data(collab))
    .unwrap();
  let uid = app_client.uid().await;
  let folder = Folder::from_collab_doc_state(
    uid,
    CollabOrigin::Client(CollabClient::new(uid, app_client.device_id.clone())),
    encoded_collab.into(),
    &workspace_id,
    vec![],
  )
  .unwrap();
  assert!(folder
    .get_my_trash_sections()
    .iter()
    .any(|v| v.id == view_id_to_be_deleted.clone()));
  web_client
    .api_client
    .get_workspace_trash(&workspace_id)
    .await
    .unwrap()
    .views
    .iter()
    .any(|v| v.view.view_id == view_id_to_be_deleted.clone());
}
