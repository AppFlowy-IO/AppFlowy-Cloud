use crate::appflowy_ai_client;

use appflowy_ai_client::dto::{AIModel, STREAM_ANSWER_KEY};
use appflowy_ai_client::error::AIError;
use futures::stream::StreamExt;
use infra::reqwest::JsonStream;

#[tokio::test]
async fn qa_test() {
  let client = appflowy_ai_client();
  client.health_check().await.unwrap();
  let chat_id = uuid::Uuid::new_v4().to_string();
  let resp = client
    .send_question(&chat_id, 1, "I feel hungry", &AIModel::GPT4o, None)
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
