use crate::collab::util::empty_document_editor;
use client_api_test::TestClient;
use collab_entity::CollabType;
use database_entity::dto::CreateCollabParams;
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test]
async fn query_collab_embedding_after_create_test() {
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

  let test_client = TestClient::new_user().await;
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

#[tokio::test]
async fn document_full_sync_then_search_test() {
  let object_id = Uuid::new_v4().to_string();
  let mut local_document = empty_document_editor(&object_id);
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let doc_state = local_document.encode_collab().encode_to_bytes().unwrap();
  let params = CreateCollabParams {
    workspace_id: workspace_id.clone(),
    object_id: object_id.clone(),
    encoded_collab_v1: doc_state,
    collab_type: CollabType::Document,
  };
  test_client.api_client.create_collab(params).await.unwrap();

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
  local_document.insert_paragraphs(contents.into_iter().map(|s| s.to_string()).collect());
  let encode_collab = local_document.encode_collab();

  // After full sync, two document should be the same
  test_client
    .api_client
    .collab_full_sync(
      &workspace_id,
      &object_id,
      CollabType::Document,
      encode_collab.doc_state.to_vec(),
      encode_collab.state_vector.to_vec(),
    )
    .await
    .unwrap();

  let remote_document = test_client
    .create_document_collab(&workspace_id, &object_id)
    .await;
  let remote_plain_text = remote_document.to_plain_text(false).unwrap();
  let local_plain_text = local_document.to_plain_text();
  assert_eq!(local_plain_text, remote_plain_text);

  // Retry until the result is ok, with a timeout of 30 seconds
  let result = timeout(Duration::from_secs(30), async {
    loop {
      let response = test_client
        .api_client
        .search_documents(&workspace_id, "workflows", 1, 200)
        .await
        .unwrap();

      if response.is_empty() {
        tokio::time::sleep(Duration::from_millis(500)).await;
        continue;
      } else {
        return response;
      }
    }
  })
  .await;

  // Ensure the timeout didn't occur and the result is ok
  match result {
    Ok(search_result) => {
      assert_eq!(search_result.len(), 1);
      assert_eq!(search_result[0].preview, Some("AppFlowy is an open-source project.It is an alternative to tools like Notion.AppFlowy provides full control of your data.The project is built using Flutter for the frontend.Rust powers AppFlowy's back".to_string()));
    },
    Err(_) => panic!("Test failed: Timeout after 30 seconds."),
  }
}
