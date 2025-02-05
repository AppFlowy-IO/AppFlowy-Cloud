use crate::appflowy_ai_client;
use appflowy_ai_client::dto::CreateChatContext;
#[tokio::test]
async fn create_chat_context_test() {
  let client = appflowy_ai_client();
  let chat_id = uuid::Uuid::new_v4().to_string();
  let context = CreateChatContext {
    chat_id: chat_id.clone(),
    context_loader: "text".to_string(),
    content: "I have lived in the US for five years".to_string(),
    chunk_size: 1000,
    chunk_overlap: 20,
    metadata: Default::default(),
  };
  client.create_chat_text_context(context).await.unwrap();
  let resp = client
    .send_question(
      &uuid::Uuid::new_v4().to_string(),
      &chat_id,
      1,
      "Where I live?",
      "gpt-4o-mini",
      None,
    )
    .await
    .unwrap();
  // response will be something like:
  // Based on the context you provided, you have lived in the US for five years. Therefore, it is likely that you currently live in the US
  assert!(!resp.content.is_empty());
}
