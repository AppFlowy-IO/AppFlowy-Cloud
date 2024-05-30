use client_api_test::TestClient;
use database_entity::dto::{ChatMessage, CreateChatMessageParams, CreateChatParams, MessageCursor};
use futures_util::StreamExt;

#[tokio::test]
async fn create_chat_and_create_messages_test() {
  let test_client = TestClient::new_user_without_ws_conn().await;
  let workspace_id = test_client.workspace_id().await;

  let chat_id = uuid::Uuid::new_v4().to_string();
  let params = CreateChatParams {
    chat_id: chat_id.clone(),
    name: "my first chat".to_string(),
    rag_ids: vec![],
  };

  test_client
    .api_client
    .create_chat(&workspace_id, params)
    .await
    .unwrap();

  let mut messages = vec![];
  for i in 0..10 {
    let params = CreateChatMessageParams::new_system(format!("hello world {}", i));
    let message = test_client
      .api_client
      .create_chat_message(&workspace_id, &chat_id, params)
      .await
      .unwrap()
      .next()
      .await
      .unwrap()
      .unwrap();
    messages.push(message);
  }
  // DESC is the default order
  messages.reverse();

  // get messages before third message. it should return first two messages even though we asked
  // for 10 messages
  assert_eq!(messages[7].content, "hello world 2");
  let message_before_third = test_client
    .api_client
    .get_chat_messages(
      &workspace_id,
      &chat_id,
      MessageCursor::BeforeMessageId(messages[7].message_id),
      10,
    )
    .await
    .unwrap();
  assert!(!message_before_third.has_more);
  assert_eq!(message_before_third.messages.len(), 2);
  assert_eq!(message_before_third.messages[0].content, "hello world 1");
  assert_eq!(message_before_third.messages[1].content, "hello world 0");

  // get message after third message
  assert_eq!(messages[2].content, "hello world 7");
  let message_after_third = test_client
    .api_client
    .get_chat_messages(
      &workspace_id,
      &chat_id,
      MessageCursor::AfterMessageId(messages[2].message_id),
      2,
    )
    .await
    .unwrap();
  assert!(!message_after_third.has_more);
  assert_eq!(message_after_third.messages.len(), 2);
  assert_eq!(message_after_third.messages[0].content, "hello world 9");
  assert_eq!(message_after_third.messages[1].content, "hello world 8");

  let next_back = test_client
    .api_client
    .get_chat_messages(&workspace_id, &chat_id, MessageCursor::NextBack, 3)
    .await
    .unwrap();
  assert!(next_back.has_more);
  assert_eq!(next_back.messages.len(), 3);
  assert_eq!(next_back.messages[0].content, "hello world 9");
  assert_eq!(next_back.messages[1].content, "hello world 8");

  let next_back = test_client
    .api_client
    .get_chat_messages(&workspace_id, &chat_id, MessageCursor::NextBack, 100)
    .await
    .unwrap();
  assert!(!next_back.has_more);
  assert_eq!(next_back.messages.len(), 10);
}

#[tokio::test]
async fn chat_qa_test() {
  let test_client = TestClient::new_user_without_ws_conn().await;
  let workspace_id = test_client.workspace_id().await;
  let chat_id = uuid::Uuid::new_v4().to_string();
  let params = CreateChatParams {
    chat_id: chat_id.clone(),
    name: "my second chat".to_string(),
    rag_ids: vec![],
  };

  test_client
    .api_client
    .create_chat(&workspace_id, params)
    .await
    .unwrap();

  let params = CreateChatMessageParams::new_user("where is singapore?");
  let stream = test_client
    .api_client
    .create_chat_message(&workspace_id, &chat_id, params)
    .await
    .unwrap();

  let messages: Vec<ChatMessage> = stream.map(|message| message.unwrap()).collect().await;
  assert_eq!(messages.len(), 2);
}
