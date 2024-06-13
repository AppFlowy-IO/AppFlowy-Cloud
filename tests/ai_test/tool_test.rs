use appflowy_ai_client::dto::{TagItem, TagRowData, TagRowParams};
use client_api_test::TestClient;
use serde_json::json;
use shared_entity::dto::ai_dto::{SummarizeRowData, SummarizeRowParams};

#[tokio::test]
async fn summarize_row_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let params = SummarizeRowParams {
    workspace_id: workspace_id.clone(),
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
#[tokio::test]
async fn tag_row_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let params = TagRowParams {
    workspace_id: workspace_id.clone(),
    data: TagRowData {
      existing_tags: vec![],
      items: vec![TagItem {
        title: "Atomic habits".to_string(),
        content: "Atomic Habits by James Clear discusses how small, consistent changes in habits can lead to significant improvements over time. The book provides strategies for building good habits and breaking bad ones, emphasizing the importance of making tiny adjustments that compound into remarkable results. Clear introduces the concepts of habit stacking, the two-minute rule, and the four laws of behavior change: making habits obvious, attractive, easy, and satisfying".to_string(),
      }],
      num_tags: 5,
    },
  };

  let resp = test_client.api_client.tag_row(params).await.unwrap();
  assert_eq!(resp.tags.len(), 5);
}
