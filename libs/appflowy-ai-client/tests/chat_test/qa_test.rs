use crate::appflowy_ai_client;

use appflowy_ai_client::client::JsonStream;
use appflowy_ai_client::dto::{AIModel, STEAM_ANSWER_KEY};
use futures::stream::StreamExt;

#[tokio::test]
async fn qa_test() {
  let client = appflowy_ai_client();
  client.health_check().await.unwrap();
  let chat_id = uuid::Uuid::new_v4().to_string();
  let resp = client
    .send_question(&chat_id, "I feel hungry", &AIModel::GPT4o, None)
    .await
    .unwrap();
  assert!(!resp.content.is_empty());

  let questions = client
    .get_related_question(&chat_id, &1, &AIModel::GPT4oMini)
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
    .stream_question(&chat_id, "I feel hungry", None, &AIModel::GPT4oMini)
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
    .stream_question_v2(&chat_id, "I feel hungry", None, &AIModel::GPT4oMini)
    .await
    .unwrap();
  let json_stream = JsonStream::<serde_json::Value>::new(stream);

  let messages: Vec<String> = json_stream
    .filter_map(|item| async {
      match item {
        Ok(value) => value
          .get(STEAM_ANSWER_KEY)
          .and_then(|s| s.as_str().map(ToString::to_string)),
        Err(_) => None,
      }
    })
    .collect()
    .await;

  println!("final answer: {}", messages.join(""));
}

#[tokio::test]
async fn download_package_test() {
  let client = appflowy_ai_client();
  let packages = client.get_local_ai_package("macos").await.unwrap();
  assert!(!packages.0.is_empty());
  println!("packages: {:?}", packages);
}

#[tokio::test]
async fn get_local_ai_config_test() {
  let client = appflowy_ai_client();
  let config = client
    .get_local_ai_config("macos", Some("0.6.10".to_string()))
    .await
    .unwrap();
  assert!(!config.models.is_empty());

  assert!(!config.models[0].embedding_model.download_url.is_empty());
  assert!(!config.models[0].chat_model.download_url.is_empty());

  assert!(!config.plugin.version.is_empty());
  assert!(!config.plugin.url.is_empty());
  println!("packages: {:?}", config);
}
