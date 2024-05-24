use client_api_test::TestClient;
use database_entity::dto::{CreateChatMessageParams, CreateChatParams, MessageCursor};

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
      .unwrap();
    messages.push(message);
  }

  // get messages before third message. it should return first two messages even though we asked
  // for 10 messages
  let message_before_third = test_client
    .api_client
    .get_chat_messages(
      &workspace_id,
      &chat_id,
      MessageCursor::BeforeMessageId(messages[2].question.message_id),
      10,
    )
    .await
    .unwrap();

  assert!(!message_before_third.has_more);
  assert_eq!(message_before_third.messages.len(), 2);
  assert_eq!(
    message_before_third.messages[0].message_id,
    messages[0].question.message_id
  );
  assert_eq!(
    message_before_third.messages[1].message_id,
    messages[1].question.message_id
  );

  // get message after third message
  let message_after_third = test_client
    .api_client
    .get_chat_messages(
      &workspace_id,
      &chat_id,
      MessageCursor::AfterMessageId(messages[2].question.message_id),
      2,
    )
    .await
    .unwrap();
  assert!(message_after_third.has_more);
  assert_eq!(message_after_third.messages.len(), 2);
  assert_eq!(
    message_after_third.messages[0].message_id,
    messages[3].question.message_id
  );
  assert_eq!(
    message_after_third.messages[1].message_id,
    messages[4].question.message_id
  );

  // get all messages after 8th message
  let remaining_messages = test_client
    .api_client
    .get_chat_messages(
      &workspace_id,
      &chat_id,
      MessageCursor::AfterMessageId(messages[7].question.message_id),
      100,
    )
    .await
    .unwrap();

  // has_more should be false because we only have 10 messages
  assert!(!remaining_messages.has_more);
  assert_eq!(remaining_messages.messages.len(), 2);
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
  let message = test_client
    .api_client
    .create_chat_message(&workspace_id, &chat_id, params)
    .await
    .unwrap();
  assert!(!message.answer.unwrap().content.is_empty());
}
