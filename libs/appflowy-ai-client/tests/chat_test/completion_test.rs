use crate::appflowy_ai_client;
use appflowy_ai_client::dto::CompletionType;

#[tokio::test]
async fn continue_writing_test() {
  let client = appflowy_ai_client();
  let resp = client
    .completion_text("I feel hungry", CompletionType::ContinueWriting)
    .await
    .unwrap();
  assert!(!resp.text.is_empty());
  println!("{}", resp.text);
}

#[tokio::test]
async fn improve_writing_test() {
  let client = appflowy_ai_client();
  let resp = client
    .completion_text(
      "I fell tired because i sleep not very well last night",
      CompletionType::ImproveWriting,
    )
    .await
    .unwrap();

  // the response would be something like: I feel exhausted due to a restless night of sleep.
  assert!(!resp.text.is_empty());
  println!("{}", resp.text);
}
#[tokio::test]
async fn make_text_shorter_text() {
  let client = appflowy_ai_client();
  let resp = client
    .completion_text(
      "I have an immense passion and deep-seated affection for Rust, a modern, multi-paradigm, high-performance programming language that I find incredibly satisfying to use due to its focus on safety, speed, and concurrency",
      CompletionType::MakeShorter,
    )
    .await
    .unwrap();

  // the response would be something like:
  // I'm deeply passionate about Rust, a modern, high-performance programming language, due to its emphasis on safety, speed, and concurrency
  assert!(!resp.text.is_empty());
  println!("{}", resp.text);
}
