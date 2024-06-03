use crate::appflowy_ai_client;
use futures::stream::StreamExt;

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

  let questions = client
    .get_related_question(&chat_id, &1)
    .await
    .unwrap()
    .items;
  println!("questions: {:?}", questions);
  assert_eq!(questions.len(), 3)
}
#[tokio::test]
async fn stop_stream_test() {
  let client = appflowy_ai_client();
  client.health_check().await.unwrap();
  let chat_id = uuid::Uuid::new_v4().to_string();
  let mut stream = client
    .stream_question(&chat_id, "I feel hungry")
    .await
    .unwrap();

  let mut count = 0;
  while let Some(message) = stream.next().await {
    if count > 1 {
      break;
    }
    count += 1;
    println!("message: {:?}", message);
  }

  assert_ne!(count, 0);
}

#[tokio::test]
async fn stream_test() {
  let client = appflowy_ai_client();
  client.health_check().await.unwrap();
  let chat_id = uuid::Uuid::new_v4().to_string();
  let stream = client
    .stream_question(&chat_id, "I feel hungry")
    .await
    .unwrap();

  let messages: Vec<String> = stream.map(|message| message.unwrap()).collect().await;
  println!("final answer: {}", messages.join(""));
}
