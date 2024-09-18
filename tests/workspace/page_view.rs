use client_api_test::generate_unique_registered_user_client;
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
  let general_workspace = &folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap();
  let todo = general_workspace
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
  let getting_started = general_workspace
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
