use client_api_test::TestClient;
use shared_entity::dto::import_dto::ImportTaskStatus;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::test]
async fn import_notion_zip_test() {
  let client = TestClient::new_user().await;

  let file_path = PathBuf::from("tests/workspace/asset/project&task.zip");
  client.api_client.import_file(&file_path).await.unwrap();

  // when importing a file, the workspace for the file should be created and it's
  // not visible until the import task is completed
  let workspaces = client.api_client.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);

  let tasks = client.api_client.get_import_list().await.unwrap().tasks;
  assert_eq!(tasks.len(), 1);
  assert_eq!(tasks[0].status, ImportTaskStatus::Pending);

  let mut task_completed = false;
  let max_retries = 12;
  let mut retries = 0;
  while !task_completed && retries < max_retries {
    tokio::time::sleep(Duration::from_secs(10)).await;
    let tasks = client.api_client.get_import_list().await.unwrap().tasks;
    assert_eq!(tasks.len(), 1);

    if tasks[0].status == ImportTaskStatus::Completed {
      task_completed = true;
    }
    retries += 1;
  }

  assert!(
    task_completed,
    "The import task was not completed within the expected time."
  );

  // after the import task is completed, the new workspace should be visible
  let workspaces = client.api_client.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 2);

  // check the imported workspace
  // let imported_workspace_id = workspaces[1].workspace_id.to_string();
  // let folder = client.get_folder(&imported_workspace_id).await;
  // let view = folder.get_views_belong_to(&imported_workspace_id);
  // assert_eq!(view.len(), 1);
}
