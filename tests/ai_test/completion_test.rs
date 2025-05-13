use appflowy_ai_client::dto::{CompleteTextParams, CompletionMetadata, CompletionType};
use client_api_test::{ai_test_enabled, collect_completion_v2, TestClient};

#[tokio::test]
async fn generate_chat_message_answer_test() {
  if !ai_test_enabled() {
    return;
  }
  let test_client = TestClient::new_user_without_ws_conn().await;
  let workspace_id = test_client.workspace_id().await;
  let doc_id = uuid::Uuid::new_v4();

  let params = CompleteTextParams {
    text: "I seen the movie last night and it was amazing".to_string(),
    completion_type: Some(CompletionType::SpellingAndGrammar),
    metadata: Some(CompletionMetadata {
      object_id: doc_id,
      workspace_id: Some(workspace_id),
      rag_ids: None,
      completion_history: None,
      custom_prompt: None,
      prompt_id: None,
    }),
    format: Default::default(),
  };

  let stream = test_client
    .api_client
    .stream_completion_v2(&workspace_id, params, None)
    .await
    .unwrap();
  let (answer, comment) = collect_completion_v2(stream).await;
  println!("answer: {}", answer);
  println!("comment: {}", comment);
  assert!(!answer.is_empty());
  assert!(!comment.is_empty());
}
