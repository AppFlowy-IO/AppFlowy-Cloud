use client_api_test::TestClient;
use collab::core::collab::MutexCollab;
use collab::preclude::Collab;
use collab_document::document::Document;
use collab_entity::CollabType;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use workspace_template::document::get_started::get_started_document_data;

#[tokio::test]
async fn test_document_indexing_and_search() {
  // Set up all the required data
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = uuid::Uuid::new_v4().to_string();

  let collab_type = CollabType::Document;
  let encoded_collab = {
    let document_data = get_started_document_data().unwrap();
    let collab = Arc::new(MutexCollab::new(Collab::new(
      test_client.uid().await,
      object_id.clone(),
      test_client.device_id.clone(),
      vec![],
      false,
    )));
    let document = Document::create_with_data(collab, document_data).unwrap();
    document.encode_collab().unwrap()
  };
  test_client
    .create_and_edit_collab_with_data(
      &object_id,
      &workspace_id,
      collab_type.clone(),
      Some(encoded_collab),
    )
    .await;
  test_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  sleep(Duration::from_millis(2000)).await;

  // document should get automatically indexed after opening if it wasn't indexed before

  let search_resp = test_client
    .api_client
    .search_documents(&workspace_id, "Appflowy", 1, 20)
    .await
    .unwrap();
  assert_eq!(search_resp.len(), 1);
  let item = &search_resp[0];
  assert_eq!(item.object_id, object_id);
  assert_eq!(item.preview.as_deref(), Some("\nWelcome to AppFlowy"));
}
