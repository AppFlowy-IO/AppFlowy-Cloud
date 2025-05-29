use crate::ai_test::util::extract_image_url;
use std::time::Duration;

use appflowy_ai_client::dto::{
  ChatQuestionQuery, OutputContent, OutputContentMetadata, OutputLayout, ResponseFormat,
};
use client_api::entity::{QuestionStream, QuestionStreamValue};
use client_api_test::{ai_test_enabled, TestClient};
use futures_util::StreamExt;
use serde_json::json;
use shared_entity::dto::chat_dto::{
  CreateAnswerMessageParams, CreateChatMessageParams, CreateChatParams, MessageCursor,
  UpdateChatParams,
};
use uuid::Uuid;

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
  let rag1 = Uuid::new_v4();
  let rag2 = Uuid::new_v4();
  test_client
    .api_client
    .update_chat_settings(
      &workspace_id,
      &chat_id,
      UpdateChatParams {
        name: Some("my second chat".to_string()),
        metadata: None,
        rag_ids: Some(vec![rag1.to_string(), rag2.to_string()]),
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
  assert_eq!(settings.rag_ids, vec![rag1.to_string(), rag2.to_string()]);

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
  let answer = collect_answer(answer_stream, None).await;
  assert!(!answer.is_empty());
}

// #[tokio::test]
// async fn stop_streaming_test() {
//   if !ai_test_enabled() {
//     return;
//   }
//   let test_client = TestClient::new_user_without_ws_conn().await;
//   let workspace_id = test_client.workspace_id().await;
//   let chat_id = uuid::Uuid::new_v4().to_string();
//   let params = CreateChatParams {
//     chat_id: chat_id.clone(),
//     name: "Stop streaming test".to_string(),
//     rag_ids: vec![],
//   };
//
//   test_client
//     .api_client
//     .create_chat(&workspace_id, params)
//     .await
//     .unwrap();
//   let params = CreateChatMessageParams::new_user("when to use js");
//   let question = test_client
//     .api_client
//     .create_question(&workspace_id, &chat_id, params)
//     .await
//     .unwrap();
//   let answer_stream = test_client
//     .api_client
//     .stream_answer_v2(&workspace_id, &chat_id, question.message_id)
//     .await
//     .unwrap();
//   let answer = collect_answer(answer_stream, Some(1)).await;
//   println!("answer:\n{}", answer);
//   assert!(!answer.is_empty());
// }

#[tokio::test]
async fn get_format_question_message_test() {
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

  let params = CreateChatMessageParams::new_user(
    "what is the different between Rust and c++? Give me three points",
  );
  let question = test_client
    .api_client
    .create_question(&workspace_id, &chat_id, params)
    .await
    .unwrap();

  let query = ChatQuestionQuery {
    chat_id,
    question_id: question.message_id,
    format: ResponseFormat {
      output_layout: OutputLayout::SimpleTable,
      output_content: OutputContent::TEXT,
      output_content_metadata: None,
    },
  };

  let answer_stream = test_client
    .api_client
    .stream_answer_v3(&workspace_id, query, None)
    .await
    .unwrap();
  let answer = collect_answer(answer_stream, None).await;
  println!("answer:\n{}", answer);
  assert!(!answer.is_empty());
}

#[tokio::test]
async fn get_text_with_image_message_test() {
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

  let params = CreateChatMessageParams::new_user(
    "I have a little cat. It is black with big eyes, short legs and a long tail",
  );
  let question = test_client
    .api_client
    .create_question(&workspace_id, &chat_id, params)
    .await
    .unwrap();

  let query = ChatQuestionQuery {
    chat_id: chat_id.clone(),
    question_id: question.message_id,
    format: ResponseFormat {
      output_layout: OutputLayout::SimpleTable,
      output_content: OutputContent::RichTextImage,
      output_content_metadata: Some(OutputContentMetadata {
        custom_image_prompt: None,
        image_model: "dall-e-3".to_string(),
        size: None,
        quality: None,
      }),
    },
  };

  let answer_stream = test_client
    .api_client
    .stream_answer_v3(&workspace_id, query, None)
    .await
    .unwrap();
  let answer = collect_answer(answer_stream, None).await;
  println!("answer:\n{}", answer);
  let image_url = extract_image_url(&answer).unwrap();
  let (workspace_id_2, chat_id_2, file_id_2) = test_client
    .api_client
    .parse_blob_url_v1(&image_url)
    .unwrap();
  assert_eq!(workspace_id, workspace_id_2);
  assert_eq!(chat_id, chat_id_2);

  let mut retries = 6;
  let retry_interval = Duration::from_secs(20);
  let mut last_error = None;

  // The image will be generated in the background, so we need to retry until it's available
  while retries > 0 {
    match test_client
      .api_client
      .get_blob_v1(&workspace_id_2, &chat_id_2, &file_id_2)
      .await
    {
      Ok(_) => {
        // Success, exit the loop
        last_error = None;
        break;
      },
      Err(err) => {
        last_error = Some(err);
        retries -= 1;
      },
    }

    if retries > 0 {
      tokio::time::sleep(retry_interval).await;
    }
  }

  if let Some(err) = last_error {
    panic!(
      "Failed to get blob after retries: {:?}, url:{}",
      err, image_url
    );
  }

  assert!(!answer.is_empty());
}

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

  test_client
    .api_client
    .save_answer(
      &workspace_id,
      &chat_id,
      CreateAnswerMessageParams {
        content: answer.content,
        metadata: None,
        question_message_id: question.message_id,
      },
    )
    .await
    .unwrap();

  let find_question = test_client
    .api_client
    .get_question_message_from_answer_id(&workspace_id, &chat_id, answer.message_id)
    .await
    .unwrap()
    .unwrap();

  assert_eq!(find_question.reply_message_id.unwrap(), answer.message_id);
}

#[tokio::test]
async fn get_model_list_test() {
  if !ai_test_enabled() {
    return;
  }
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let models = test_client
    .api_client
    .get_model_list(&workspace_id)
    .await
    .unwrap()
    .models;
  assert!(!models.is_empty());
  assert!(models.len() >= 2, "models.len() = {}", models.len());
  println!("models: {:?}", models);
}

async fn collect_answer(
  mut stream: QuestionStream,
  stop_when_num_of_char: Option<usize>,
) -> String {
  let mut answer = String::new();
  let mut num_of_char: usize = 0;
  while let Some(value) = stream.next().await {
    num_of_char += match value.unwrap() {
      QuestionStreamValue::Answer { value } => {
        answer.push_str(&value);
        value.len()
      },
      _ => 0,
    };
    if let Some(stop_when_num_of_char) = stop_when_num_of_char {
      if num_of_char >= stop_when_num_of_char {
        break;
      }
    }
  }
  answer
}
