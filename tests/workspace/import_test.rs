use anyhow::Error;
use client_api_test::TestClient;
use collab_document::importer::define::URL_FIELD;
use collab_folder::ViewLayout;

use collab_database::database::get_inline_view_id;
use collab_document::blocks::BlockType;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn import_blog_post_test() {
  // Step 1: Import the blog post zip
  let (client, imported_workspace_id) = import_notion_zip_until_complete("blog_post.zip").await;
  let uid = client.uid().await;

  // Step 2: Fetch the folder and views
  let folder = client.get_folder(imported_workspace_id).await;
  let mut space_views = folder.get_views_belong_to(&imported_workspace_id.to_string(), uid);
  assert_eq!(
    space_views.len(),
    1,
    "Expected 1 view, found {:?}",
    space_views
  );

  // Step 3: Validate the space view name
  let space_view = space_views.pop().unwrap();
  assert_eq!(space_view.name, "Imported Space");

  // Step 4: Fetch the imported view and document
  let imported_view = folder
    .get_views_belong_to(&space_view.id, uid)
    .pop()
    .unwrap();
  let document = client
    .get_document(imported_workspace_id, imported_view.id.parse().unwrap())
    .await;

  // Step 5: Generate the expected blob URLs
  let host = client.api_client.base_url.clone();
  let object_id = imported_view.id.clone();
  let blob_names = vec![
    "PGTRCFsf2duc7iP3KjE62Xs8LE7B96a0aQtLtGtfIcw=.jpg",
    "fFWPgqwdqbaxPe7Q_vUO143Sa2FypnRcWVibuZYdkRI=.jpg",
    "EIj9Z3yj8Gw8UW60U8CLXx7ulckEs5Eu84LCFddCXII=.jpg",
  ];

  let mut expected_urls = blob_names
    .iter()
    .map(|s| format!("{host}/api/file_storage/{imported_workspace_id}/v1/blob/{object_id}/{s}"))
    .collect::<Vec<String>>();

  // Step 6: Concurrently fetch blobs
  let fetch_blob_futures = blob_names.into_iter().map(|blob_name| {
    client
      .api_client
      .get_blob_v1(&imported_workspace_id, &object_id, blob_name)
  });

  let blob_results = futures::future::join_all(fetch_blob_futures).await;

  // Ensure all blobs are fetched successfully
  for result in blob_results {
    result.unwrap();
  }

  // Step 7: Extract block URLs from the document and filter expected URLs
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

  // Step 8: Ensure no expected URLs remain
  assert!(
    expected_urls.is_empty(),
    "expected URLs to be empty: {:?}",
    expected_urls
  );
}

#[tokio::test]
async fn import_project_and_task_zip_test() {
  let (client, imported_workspace_id) = import_notion_zip_until_complete("project&task.zip").await;
  let uid = client.uid().await;
  let folder = client.get_folder(imported_workspace_id).await;
  let workspace_database = client.get_workspace_database(imported_workspace_id).await;
  let space_views = folder.get_views_belong_to(&imported_workspace_id.to_string(), uid);
  assert_eq!(
    space_views.len(),
    1,
    "Expected 1 view, found {:?}",
    space_views
  );
  assert_eq!(space_views[0].name, "Imported Space");
  assert!(space_views[0].space_info().is_some());

  let mut sub_views = folder.get_views_belong_to(&space_views[0].id, uid);
  let imported_view = sub_views.pop().unwrap();
  assert_eq!(imported_view.name, "Projects & Tasks");
  assert_eq!(
    imported_view.children.len(),
    2,
    "Expected 2 views, found {:?}",
    imported_view.children
  );
  assert_eq!(imported_view.layout, ViewLayout::Document);

  let sub_views = folder.get_views_belong_to(&imported_view.id, uid);
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
        .get_database(imported_workspace_id, &database_id)
        .await;
      let inline_views = get_inline_view_id(&database).unwrap();
      let fields = database.get_fields_in_view(&inline_views, None);
      let rows = database.collect_all_rows(false).await;
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
        .get_database(imported_workspace_id, &database_id)
        .await;
      let inline_views = get_inline_view_id(&database).unwrap();
      let fields = database.get_fields_in_view(&inline_views, None);
      let rows = database.collect_all_rows(false).await;
      assert_eq!(rows.len(), 17);
      assert_eq!(fields.len(), 13);
      continue;
    }

    panic!("Unexpected view found: {:?}", view);
  }
}

#[tokio::test]
async fn imported_workspace_do_not_become_latest_visit_workspace_test() {
  let client = TestClient::new_user().await;
  let file_path = PathBuf::from("tests/workspace/asset/blog_post.zip".to_string());
  client.api_client.import_file(&file_path).await.unwrap();

  // When importing a Notion file, a new task is spawned to create a workspace for the imported data.
  // However, the workspace should remain hidden until the import is completed successfully.
  let user_workspace = client.get_user_workspace_info().await;
  let visiting_workspace_id = user_workspace.visiting_workspace.workspace_id;
  assert_eq!(user_workspace.workspaces.len(), 1);
  assert_eq!(
    user_workspace.visiting_workspace.workspace_id,
    user_workspace.workspaces[0].workspace_id
  );

  wait_until_num_import_task_complete(&client, 1).await;

  // after the workspace was imported, then the workspace should be visible
  let user_workspace = client.get_user_workspace_info().await;
  assert_eq!(user_workspace.workspaces.len(), 2);
  assert_eq!(
    user_workspace.visiting_workspace.workspace_id,
    visiting_workspace_id,
  );
}

#[allow(dead_code)]
async fn upload_file(
  client: &TestClient,
  name: &str,
  upload_after_secs: Option<u64>,
) -> Result<(), Error> {
  let file_path = PathBuf::from(format!("tests/workspace/asset/{name}"));
  let url = client
    .api_client
    .create_import(&file_path)
    .await?
    .presigned_url;

  if let Some(secs) = upload_after_secs {
    tokio::time::sleep(Duration::from_secs(secs)).await;
  }

  client
    .api_client
    .upload_import_file(&file_path, &url)
    .await?;
  Ok(())
}

// upload_after_secs: simulate the delay of uploading the file
async fn import_notion_zip_until_complete(name: &str) -> (TestClient, Uuid) {
  let client = TestClient::new_user().await;

  // Uncomment the following lines to use the predicated upload file API.
  // Currently, we use `upload_file` to send a file to appflowy_worker, which then
  // processes the upload task.
  let file_path = PathBuf::from(format!("tests/workspace/asset/{name}"));
  client.api_client.import_file(&file_path).await.unwrap();
  // upload_file(&client, name, None).await.unwrap();

  let default_workspace_id = client.workspace_id().await;

  // when importing a file, the workspace for the file should be created and it's
  // not visible until the import task is completed
  let workspaces = client.api_client.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let tasks = client.api_client.get_import_list().await.unwrap().tasks;
  assert_eq!(tasks.len(), 1);
  assert_eq!(tasks[0].status, 0);

  wait_until_num_import_task_complete(&client, 1).await;

  // after the import task is completed, the new workspace should be visible
  let workspaces = client.api_client.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 2);

  let imported_workspace = workspaces
    .into_iter()
    .find(|workspace| workspace.workspace_id != default_workspace_id)
    .expect("Failed to find imported workspace");

  let imported_workspace_id = imported_workspace.workspace_id;
  (client, imported_workspace_id)
}

async fn wait_until_num_import_task_complete(client: &TestClient, num: usize) {
  let mut task_completed = false;
  let max_retries = 12;
  let mut retries = 0;
  while !task_completed {
    tokio::time::sleep(Duration::from_secs(10)).await;
    let tasks = client.api_client.get_import_list().await.unwrap().tasks;
    assert_eq!(tasks.len(), num);
    if tasks[0].status == 1 {
      task_completed = true;
    }
    retries += 1;

    if retries > max_retries {
      eprintln!("{:?}", tasks);
      break;
    }
  }

  assert!(
    task_completed,
    "The import task was not completed within the expected time."
  );
}
