use client_api_test::TestClient;
use collab_document::importer::define::{BlockType, URL_FIELD};
use collab_folder::ViewLayout;
use shared_entity::dto::import_dto::ImportTaskStatus;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::test]
async fn import_blog_post_test() {
  let (client, imported_workspace_id) = import_zip("blog_post.zip").await;
  let folder = client.get_folder(&imported_workspace_id).await;
  let mut space_views = folder.get_views_belong_to(&imported_workspace_id);
  assert_eq!(
    space_views.len(),
    1,
    "Expected 1 view, found {:?}",
    space_views
  );

  let space_view = space_views.pop().unwrap();
  assert_eq!(space_view.name, "Imported Space");
  let imported_view = folder.get_views_belong_to(&space_view.id).pop().unwrap();

  let document = client
    .get_document(&imported_workspace_id, &imported_view.id)
    .await;

  let host = client.api_client.base_url.clone();
  let object_id = imported_view.id.clone();
  let mut expected_urls = vec![
    "PGTRCFsf2duc7iP3KjE62Xs8LE7B96a0aQtLtGtfIcw=.jpg",
    "fFWPgqwdqbaxPe7Q_vUO143Sa2FypnRcWVibuZYdkRI=.jpg",
    "EIj9Z3yj8Gw8UW60U8CLXx7ulckEs5Eu84LCFddCXII=.jpg",
  ]
  .into_iter()
  .map(|s| format!("{host}/{imported_workspace_id}/v1/blob/{object_id}/{s}"))
  .collect::<Vec<String>>();

  let page_block_id = document.get_page_id().unwrap();
  let block_ids = document.get_block_children_ids(&page_block_id);
  for block_id in block_ids.iter() {
    if let Some((block_type, block_data)) = document.get_block_data(block_id) {
      if matches!(block_type, BlockType::Image) {
        let url = block_data.get(URL_FIELD).unwrap().as_str().unwrap();
        expected_urls.retain(|allowed_url| !url.contains(allowed_url));
      }
    }
  }
  println!("{:?}", expected_urls);
  assert!(expected_urls.is_empty());
}

#[tokio::test]
async fn import_project_and_task_zip_test() {
  let (client, imported_workspace_id) = import_zip("project&task.zip").await;
  let folder = client.get_folder(&imported_workspace_id).await;
  let workspace_database = client.get_workspace_database(&imported_workspace_id).await;
  let space_views = folder.get_views_belong_to(&imported_workspace_id);
  assert_eq!(
    space_views.len(),
    1,
    "Expected 1 view, found {:?}",
    space_views
  );
  assert_eq!(space_views[0].name, "Imported Space");
  assert!(space_views[0].space_info().is_some());

  let mut sub_views = folder.get_views_belong_to(&space_views[0].id);
  let imported_view = sub_views.pop().unwrap();
  assert_eq!(imported_view.name, "Projects & Tasks");
  assert_eq!(
    imported_view.children.len(),
    2,
    "Expected 2 views, found {:?}",
    imported_view.children
  );
  assert_eq!(imported_view.layout, ViewLayout::Document);

  let sub_views = folder.get_views_belong_to(&imported_view.id);
  for (index, view) in sub_views.iter().enumerate() {
    if index == 0 {
      assert_eq!(view.name, "Projects");
      assert_eq!(view.layout, ViewLayout::Grid);

      let database_id = workspace_database
        .get_database_meta_with_view_id(&view.id)
        .unwrap()
        .database_id
        .clone();
      let database = client
        .get_database(&imported_workspace_id, &database_id)
        .await;
      let inline_views = database.get_inline_view_id();
      let fields = database.get_fields_in_view(&inline_views, None);
      let rows = database.collect_all_rows().await;
      assert_eq!(rows.len(), 4);
      assert_eq!(fields.len(), 13);

      continue;
    }

    if index == 1 {
      assert_eq!(view.name, "Tasks");
      assert_eq!(view.layout, ViewLayout::Grid);

      let database_id = workspace_database
        .get_database_meta_with_view_id(&view.id)
        .unwrap()
        .database_id
        .clone();
      let database = client
        .get_database(&imported_workspace_id, &database_id)
        .await;
      let inline_views = database.get_inline_view_id();
      let fields = database.get_fields_in_view(&inline_views, None);
      let rows = database.collect_all_rows().await;
      assert_eq!(rows.len(), 17);
      assert_eq!(fields.len(), 13);
      continue;
    }

    panic!("Unexpected view found: {:?}", view);
  }
}

async fn import_zip(name: &str) -> (TestClient, String) {
  let client = TestClient::new_user().await;

  let file_path = PathBuf::from(format!("tests/workspace/asset/{name}"));
  client.api_client.import_file(&file_path).await.unwrap();
  let default_workspace_id = client.workspace_id().await;

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

  let imported_workspace = workspaces
    .into_iter()
    .find(|workspace| workspace.workspace_id.to_string() != default_workspace_id)
    .expect("Failed to find imported workspace");

  let imported_workspace_id = imported_workspace.workspace_id.to_string();
  (client, imported_workspace_id)
}
