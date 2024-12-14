use crate::collab::util::empty_document_editor;
use client_api_test::TestClient;
use collab_entity::CollabType;
use database_entity::dto::CreateCollabParams;
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test]
async fn query_collab_embedding_test() {
  let object_id = Uuid::new_v4().to_string();
  let mut editor = empty_document_editor(&object_id);
  let contents = vec![
    "AppFlowy is an open-source project.",
    "It is an alternative to tools like Notion.",
    "AppFlowy provides full control of your data.",
    "The project is built using Flutter for the frontend.",
    "Rust powers AppFlowy's backend for safety and performance.",
    "AppFlowy supports both personal and collaborative workflows.",
    "It is customizable and self-hostable.",
    "Users can create documents, databases, and workflows with AppFlowy.",
    "The community contributes actively to AppFlowy's development.",
    "AppFlowy aims to be fast, reliable, and feature-rich.",
  ];
  editor.insert_paragraphs(contents.into_iter().map(|s| s.to_string()).collect());

  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let doc_state = editor.encode_collab().encode_to_bytes().unwrap();
  let params = CreateCollabParams {
    workspace_id: workspace_id.clone(),
    object_id: object_id.clone(),
    encoded_collab_v1: doc_state,
    collab_type: CollabType::Document,
  };
  test_client.api_client.create_collab(params).await.unwrap();

  // Retry until the result is ok, with a timeout of 30 seconds
  let result = timeout(Duration::from_secs(30), async {
    loop {
      let response = test_client
        .api_client
        .get_collab_embed_info(&workspace_id, &object_id, CollabType::Document)
        .await;

      if response.is_ok() {
        return response; // Return the successful response
      }

      println!("error: {:?}", response.unwrap_err());
      tokio::time::sleep(Duration::from_millis(500)).await;
    }
  })
  .await;

  // Ensure the timeout didn't occur and the result is ok
  match result {
    Ok(Ok(_)) => println!("Test passed: Collab info retrieved successfully."),
    Ok(Err(e)) => panic!("Test failed: API returned an error: {:?}", e),
    Err(_) => panic!("Test failed: Timeout after 30 seconds."),
  }
}
