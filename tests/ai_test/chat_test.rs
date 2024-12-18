use crate::ai_test::util::read_text_from_asset;

use assert_json_diff::{assert_json_eq, assert_json_include};
use client_api::entity::{QuestionStream, QuestionStreamValue};
use client_api_test::{ai_test_enabled, TestClient};
use futures_util::StreamExt;
use serde_json::json;
use shared_entity::dto::chat_dto::{
  ChatMessageMetadata, ChatRAGData, CreateChatMessageParams, CreateChatParams, MessageCursor,
  UpdateChatParams,
};

#[tokio::test]
async fn update_chat_settings_test() {
  if !ai_test_enabled() {
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

  // Update name and rag_ids
  test_client
    .api_client
    .update_chat_settings(
      &workspace_id,
      &chat_id,
      UpdateChatParams {
        name: Some("my second chat".to_string()),
        metadata: None,
        rag_ids: Some(vec!["rag1".to_string(), "rag2".to_string()]),
      },
    )
    .await
    .unwrap();

  // Get chat settings and check if the name and rag_ids are updated
  let settings = test_client
    .api_client
    .get_chat_settings(&workspace_id, &chat_id)
    .await
    .unwrap();
  assert_eq!(settings.name, "my second chat");
  assert_eq!(
    settings.rag_ids,
    vec!["rag1".to_string(), "rag2".to_string()]
  );

  // Update chat metadata
  test_client
    .api_client
    .update_chat_settings(
      &workspace_id,
      &chat_id,
      UpdateChatParams {
        name: None,
        metadata: Some(json!({"1": "A"})),
        rag_ids: None,
      },
    )
    .await
    .unwrap();
  test_client
    .api_client
    .update_chat_settings(
      &workspace_id,
      &chat_id,
      UpdateChatParams {
        name: None,
        metadata: Some(json!({"2": "B"})),
        rag_ids: None,
      },
    )
    .await
    .unwrap();

  // check if the metadata is updated
  let settings = test_client
    .api_client
    .get_chat_settings(&workspace_id, &chat_id)
    .await
    .unwrap();
  assert_eq!(settings.metadata, json!({"1": "A", "2": "B"}));
}

#[tokio::test]
async fn create_chat_and_create_messages_test() {
  if !ai_test_enabled() {
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
    let question = test_client
      .api_client
      .create_question(&workspace_id, &chat_id, params)
      .await
      .unwrap();
    messages.push(question);
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
  if !ai_test_enabled() {
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
    data: ChatRAGData::new_text(content),
    id: "123".to_string(),
    name: "test context".to_string(),
    source: "user added".to_string(),
    extra: Some(json!({"created_at": 123})),
  };

  let params = CreateChatMessageParams::new_user("Where lucas live?").with_metadata(metadata);
  let question = test_client
    .api_client
    .create_question(&workspace_id, &chat_id, params)
    .await
    .unwrap();
  assert_json_include!(
    actual: question.meta_data,
    expected: json!([
      {
        "id": "123",
        "name": "test context",
        "source": "user added",
        "extra": {
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
  if !ai_test_enabled() {
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
  let params = CreateChatMessageParams::new_user("Hello");
  let question = test_client
    .api_client
    .create_question(&workspace_id, &chat_id, params)
    .await
    .unwrap();
  let answer_stream = test_client
    .api_client
    .stream_answer_v2(&workspace_id, &chat_id, question.message_id)
    .await
    .unwrap();
  let answer = collect_answer(answer_stream).await;
  assert!(!answer.is_empty());
}

#[tokio::test]
async fn create_chat_context_test() {
  if !ai_test_enabled() {
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

  let content = "Lacus have lived in the US for five years".to_string();
  let metadata = ChatMessageMetadata {
    data: ChatRAGData::from_text(content),
    id: chat_id.clone(),
    name: "".to_string(),
    source: "appflowy".to_string(),
    extra: None,
  };

  let params = CreateChatMessageParams::new_user("Where Lacus live?").with_metadata(metadata);
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
//   if !ai_test_enabled() {
//     return;
//   }

//   let test_client = TestClient::new_user_without_ws_conn().await;
//   let workspace_id = test_client.workspace_id().await;
//   let chat_id = uuid::Uuid::new_v4().to_string();
//   let params = CreateChatParams {
//     chat_id: chat_id.clone(),
//     name: "my second chat".to_string(),
//     rag_ids: vec![],
//   };

//   test_client
//     .api_client
//     .create_chat(&workspace_id, params)
//     .await
//     .unwrap();

//   let params = CreateChatMessageParams::new_user("where is singapore?");
//   let stream = test_client
//     .api_client
//     .create_chat_message(&workspace_id, &chat_id, params)
//     .await
//     .unwrap();
//   let messages: Vec<ChatMessage> = stream.map(|message| message.unwrap()).collect().await;
//   assert_eq!(messages.len(), 2);

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

//   let remote_messages = test_client
//     .api_client
//     .get_chat_messages(&workspace_id, &chat_id, MessageCursor::NextBack, 2)
//     .await
//     .unwrap()
//     .messages;
//   assert_eq!(remote_messages[0].content, "where is China?");
//   assert_eq!(remote_messages.len(), 2);

//   // when the question was updated, the answer should be different
//   assert_ne!(remote_messages[1].content, messages[1].content);
// }

#[tokio::test]
async fn get_question_message_test() {
  if !ai_test_enabled() {
    return;
  }

  let test_client = TestClient::new_user_without_ws_conn().await;
  let workspace_id = test_client.workspace_id().await;
  let chat_id = uuid::Uuid::new_v4().to_string();
  let params = CreateChatParams {
    chat_id: chat_id.clone(),
    name: "my ai chat".to_string(),
    rag_ids: vec![],
  };

  test_client
    .api_client
    .create_chat(&workspace_id, params)
    .await
    .unwrap();

  let params = CreateChatMessageParams::new_user("where is singapore?");
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

  assert_eq!(question.reply_message_id.unwrap(), answer.message_id);

  let find_question = test_client
    .api_client
    .get_question_message_from_answer_id(&workspace_id, &chat_id, answer.message_id)
    .await
    .unwrap()
    .unwrap();

  assert_eq!(find_question.reply_message_id.unwrap(), answer.message_id);
}

async fn collect_answer(mut stream: QuestionStream) -> String {
  let mut answer = String::new();
  while let Some(value) = stream.next().await {
    match value.unwrap() {
      QuestionStreamValue::Answer { value } => {
        answer.push_str(&value);
      },
      QuestionStreamValue::Metadata { .. } => {},
    }
  }
  answer
}
