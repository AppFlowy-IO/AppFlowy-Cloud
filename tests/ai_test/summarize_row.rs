use client_api_test::{ai_test_enabled, TestClient};
use serde_json::json;
use shared_entity::dto::ai_dto::{SummarizeRowData, SummarizeRowParams};

#[tokio::test]
async fn summarize_row_test() {
  if !ai_test_enabled() {
    return;
  }
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let params = SummarizeRowParams {
    workspace_id,
    data: SummarizeRowData::Content(
      json!({"name": "Jack", "age": 25, "city": "New York"})
        .as_object()
        .unwrap()
        .clone(),
    ),
  };

  let resp = test_client.api_client.summarize_row(params).await.unwrap();
  assert!(!resp.text.is_empty());
}
