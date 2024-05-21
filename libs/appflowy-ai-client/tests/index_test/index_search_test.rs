use appflowy_ai_client::client::AppFlowyAIClient;
use appflowy_ai_client::dto::{CollabType, Document, SearchDocumentsRequest};

#[tokio::test]
async fn index_search() {
  let client = appflowy_ai_client();

  client
    .index_documents(&[
      Document {
        id: "test-doc1".to_string(),
        doc_type: CollabType::Document,
        workspace_id: "test-workspace1".to_string(),
        content: "Relevant. This is an important test document. It should appear in results."
          .to_string(),
      },
      Document {
        id: "test-doc2".to_string(),
        doc_type: CollabType::Document,
        workspace_id: "test-workspace1".to_string(),
        content:
          "Irrelevant. This is an unimportant test document. It shouldn't appear in results."
            .to_string(),
      },
      Document {
        id: "test-doc3".to_string(),
        doc_type: CollabType::Document,
        workspace_id: "test-workspace2".to_string(),
        content:
          "Irrelevant. This is an unimportant test document. It shouldn't appear in results."
            .to_string(),
      },
    ])
    .await
    .unwrap();

  let docs = client
    .search_documents(&SearchDocumentsRequest {
      workspaces: vec!["test-workspace1".to_string()],
      query: "relevant".to_string(),
      result_count: Some(1),
    })
    .await
    .unwrap();

  assert_eq!(docs.len(), 1);
  assert_eq!(docs[0].id, "test-doc1".to_string());
}
