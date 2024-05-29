use crate::appflowy_ai_client;

#[tokio::test]
async fn qa_test() {
  let client = appflowy_ai_client();
  client.health_check().await.unwrap();
  let chat_id = uuid::Uuid::new_v4().to_string();
  let resp = client
    .send_question(&chat_id, "I feel hungry")
    .await
    .unwrap();
  assert!(!resp.content.is_empty());
}
