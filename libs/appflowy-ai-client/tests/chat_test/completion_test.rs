use crate::appflowy_ai_client;
use appflowy_ai_client::client::collect_stream_text;
use appflowy_ai_client::dto::{AIModel, CompleteTextParams, CompletionType};
#[tokio::test]
async fn continue_writing_test() {
  let client = appflowy_ai_client();
  let params = CompleteTextParams {
    text: "I feel hungry".to_string(),
    completion_type: Some(CompletionType::ImproveWriting),
    custom_prompt: None,
    metadata: None,
  };
  let stream = client
    .stream_completion_text(params, AIModel::GPT4oMini)
    .await
    .unwrap();
  let text = collect_stream_text(stream).await;
  assert!(!text.is_empty());
  println!("{}", text);
}

#[tokio::test]
async fn improve_writing_test() {
  let client = appflowy_ai_client();
  let params = CompleteTextParams {
    text: "I fell tired because i sleep not very well last night".to_string(),
    completion_type: Some(CompletionType::ImproveWriting),
    custom_prompt: None,
    metadata: None,
  };
  let stream = client
    .stream_completion_text(params, AIModel::GPT4oMini)
    .await
    .unwrap();

  let text = collect_stream_text(stream).await;

  // the response would be something like: I feel exhausted due to a restless night of sleep.
  assert!(!text.is_empty());
  println!("{}", text);
}
#[tokio::test]
async fn make_text_shorter_text() {
  let client = appflowy_ai_client();
  let params = CompleteTextParams {
        text: "I have an immense passion and deep-seated affection for Rust, a modern, multi-paradigm, high-performance programming language that I find incredibly satisfying to use due to its focus on safety, speed, and concurrency".to_string(),
        completion_type: Some(CompletionType::MakeShorter),
        custom_prompt: None,
        metadata: None,
    };
  let stream = client
    .stream_completion_text(params, AIModel::GPT4oMini)
    .await
    .unwrap();

  let text = collect_stream_text(stream).await;

  // the response would be something like:
  // I'm deeply passionate about Rust, a modern, high-performance programming language, due to its emphasis on safety, speed, and concurrency
  assert!(!text.is_empty());
  println!("{}", text);
}
