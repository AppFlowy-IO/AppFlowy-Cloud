use appflowy_ai_client::dto::{AIModel, CompletionType};
use client_api_test::{local_ai_test_enabled, TestClient};
use shared_entity::dto::ai_dto::CompleteTextParams;

#[tokio::test]
async fn improve_writing_test() {
  if !local_ai_test_enabled() {
    return;
  }
  let test_client = TestClient::new_user().await;
  test_client.api_client.set_ai_model(AIModel::GPT4o);

  let workspace_id = test_client.workspace_id().await;
  let params = CompleteTextParams {
    text: "I feel hungry".to_string(),
    completion_type: CompletionType::ImproveWriting,
  };

  let resp = test_client
    .api_client
    .completion_text(&workspace_id, params)
    .await
    .unwrap();
  assert!(!resp.text.is_empty());
}
