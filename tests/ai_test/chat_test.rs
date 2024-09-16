use crate::ai_test::util::read_text_from_asset;
use appflowy_ai_client::dto::{ChatContextLoader, CreateTextChatContext};
use assert_json_diff::assert_json_eq;
use client_api::entity::QuestionStreamValue;
use client_api_test::{local_ai_test_enabled, TestClient};
use database_entity::dto::{
  ChatMessage, ChatMessageMetadata, ChatMetadataData, CreateChatMessageParams, CreateChatParams,
  MessageCursor,
};
use futures_util::StreamExt;
use serde_json::json;

#[tokio::test]
async fn create_chat_and_create_messages_test() {
  if !local_ai_test_enabled() {
    return;
  }

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
      .create_question_answer(&workspace_id, &chat_id, params)
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
  if !local_ai_test_enabled() {
    return;
  }
  let test_client = TestClient::new_user_without_ws_conn().await;
  let workspace_id = test_client.workspace_id().await;
  let chat_id = uuid::Uuid::new_v4().to_string();
  let params = CreateChatParams {
    chat_id: chat_id.clone(),
    name: "new chat".to_string(),
    rag_ids: vec![],
  };

  test_client
    .api_client
    .create_chat(&workspace_id, params)
    .await
    .unwrap();

  let content = read_text_from_asset("my_profile.txt");
  let metadata = ChatMessageMetadata {
    data: ChatMetadataData::new_text(content),
    id: "123".to_string(),
    name: "test context".to_string(),
    source: "user added".to_string(),
    extract: Some(json!({"created_at": 123})),
  };

  let params =
    CreateChatMessageParams::new_user("Where lucas live?").with_metadata(json!(vec![metadata]));
  let question = test_client
    .api_client
    .create_question(&workspace_id, &chat_id, params)
    .await
    .unwrap();
  assert_json_eq!(
    question.meta_data,
    json!([
      {
        "id": "123",
        "name": "test context",
        "source": "user added",
        "extract": {
            "created_at": 123
        }
      }
    ])
  );

  let answer = test_client
    .api_client
    .get_answer(&workspace_id, &chat_id, question.message_id)
    .await
    .unwrap();
  assert!(answer.content.contains("Singapore"));
  assert_json_eq!(
    answer.meta_data,
    json!([
      {
        "id": "123",
        "name": "test context",
        "source": "user added",
      }
    ])
  );

  let related_questions = test_client
    .api_client
    .get_chat_related_question(&workspace_id, &chat_id, question.message_id)
    .await
    .unwrap();
  assert_eq!(related_questions.items.len(), 3);
  println!("related questions: {:?}", related_questions.items);
}

#[tokio::test]
async fn generate_chat_message_answer_test() {
  if !local_ai_test_enabled() {
    return;
  }
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
    .create_question_answer(&workspace_id, &chat_id, params)
    .await
    .unwrap();
  let messages: Vec<ChatMessage> = stream.map(|message| message.unwrap()).collect().await;
  assert_eq!(messages.len(), 2);

  let answer = test_client
    .api_client
    .get_answer(&workspace_id, &chat_id, messages[0].message_id)
    .await
    .unwrap();

  let remote_messages = test_client
    .api_client
    .get_chat_messages(&workspace_id, &chat_id, MessageCursor::NextBack, 2)
    .await
    .unwrap()
    .messages;

  assert_eq!(remote_messages.len(), 2);
  assert_eq!(remote_messages[0].message_id, answer.message_id);
}

#[tokio::test]
async fn generate_stream_answer_test() {
  if !local_ai_test_enabled() {
    return;
  }
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
  let params = CreateChatMessageParams::new_user("Teach me how to write a article?");
  let question = test_client
    .api_client
    .create_question(&workspace_id, &chat_id, params)
    .await
    .unwrap();

  // test v1 api endpoint
  let mut answer_stream = test_client
    .api_client
    .stream_answer(&workspace_id, &chat_id, question.message_id)
    .await
    .unwrap();
  let mut answer_v1 = String::new();
  while let Some(message) = answer_stream.next().await {
    let message = message.unwrap();
    let s = String::from_utf8(message.to_vec()).unwrap();
    answer_v1.push_str(&s);
  }
  assert!(!answer_v1.is_empty());

  // test v2 api endpoint
  let mut answer_stream = test_client
    .api_client
    .stream_answer_v2(&workspace_id, &chat_id, question.message_id)
    .await
    .unwrap();
  let mut answer_v2 = String::new();
  while let Some(value) = answer_stream.next().await {
    match value.unwrap() {
      QuestionStreamValue::Answer { value } => {
        answer_v2.push_str(&value);
      },
      QuestionStreamValue::Metadata { .. } => {},
    }
  }
  assert!(!answer_v2.is_empty());
}

#[tokio::test]
async fn create_chat_context_test() {
  if !local_ai_test_enabled() {
    return;
  }
  let test_client = TestClient::new_user_without_ws_conn().await;
  let workspace_id = test_client.workspace_id().await;
  let chat_id = uuid::Uuid::new_v4().to_string();
  let params = CreateChatParams {
    chat_id: chat_id.clone(),
    name: "context chat".to_string(),
    rag_ids: vec![],
  };

  test_client
    .api_client
    .create_chat(&workspace_id, params)
    .await
    .unwrap();

  let context = CreateTextChatContext {
    chat_id: chat_id.clone(),
    context_loader: ChatContextLoader::Txt,
    content: "Lacus have lived in the US for five years".to_string(),
    chunk_size: 1000,
    chunk_overlap: 20,
    metadata: Default::default(),
  };

  test_client
    .api_client
    .create_chat_context(&workspace_id, context)
    .await
    .unwrap();

  let params = CreateChatMessageParams::new_user("Where Lacus live?");
  let question = test_client
    .api_client
    .create_question(&workspace_id, &chat_id, params)
    .await
    .unwrap();

  let answer = test_client
    .api_client
    .get_answer(&workspace_id, &chat_id, question.message_id)
    .await
    .unwrap();
  println!("answer: {:?}", answer);
  if answer.content.contains("United States") {
    return;
  }
  assert!(answer.content.contains("US"));
}

// #[tokio::test]
// async fn update_chat_message_test() {
//   let test_client = TestClient::new_user_without_ws_conn().await;
//   let workspace_id = test_client.workspace_id().await;
//   let chat_id = uuid::Uuid::new_v4().to_string();
//   let params = CreateChatParams {
//     chat_id: chat_id.clone(),
//     name: "my second chat".to_string(),
//     rag_ids: vec![],
//   };
//
//   test_client
//     .api_client
//     .create_chat(&workspace_id, params)
//     .await
//     .unwrap();
//
//   let params = CreateChatMessageParams::new_user("where is singapore?");
//   let stream = test_client
//     .api_client
//     .create_chat_message(&workspace_id, &chat_id, params)
//     .await
//     .unwrap();
//   let messages: Vec<ChatMessage> = stream.map(|message| message.unwrap()).collect().await;
//   assert_eq!(messages.len(), 2);
//
//   let params = UpdateChatMessageContentParams {
//     chat_id: chat_id.clone(),
//     message_id: messages[0].message_id,
//     content: "where is China?".to_string(),
//   };
//   test_client
//     .api_client
//     .update_chat_message(&workspace_id, &chat_id, params)
//     .await
//     .unwrap();
//
//   let remote_messages = test_client
//     .api_client
//     .get_chat_messages(&workspace_id, &chat_id, MessageCursor::NextBack, 2)
//     .await
//     .unwrap()
//     .messages;
//   assert_eq!(remote_messages[0].content, "where is China?");
//   assert_eq!(remote_messages.len(), 2);
//
//   // when the question was updated, the answer should be different
//   assert_ne!(remote_messages[1].content, messages[1].content);
// }
