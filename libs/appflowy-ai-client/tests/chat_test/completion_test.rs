use crate::appflowy_ai_client;
use appflowy_ai_client::dto::{AIModel, CompletionType};
use futures::stream::StreamExt;
#[tokio::test]
async fn continue_writing_test() {
  let client = appflowy_ai_client();
  let resp = client
    .completion_text(
      "I feel hungry",
      CompletionType::ContinueWriting,
      None,
      AIModel::Claude3Sonnet,
    )
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
      None,
      AIModel::GPT4oMini,
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
  let stream = client
    .stream_completion_text(
      "I have an immense passion and deep-seated affection for Rust, a modern, multi-paradigm, high-performance programming language that I find incredibly satisfying to use due to its focus on safety, speed, and concurrency",
      CompletionType::MakeShorter,
      None,
      AIModel::GPT4oMini
    )
    .await
    .unwrap();

  let stream = stream.map(|item| {
    item.map(|bytes| {
      String::from_utf8(bytes.to_vec())
        .map(|s| s.replace('\n', ""))
        .unwrap()
    })
  });

  let lines: Vec<String> = stream.map(|message| message.unwrap()).collect().await;
  let text = lines.join("");

  // the response would be something like:
  // I'm deeply passionate about Rust, a modern, high-performance programming language, due to its emphasis on safety, speed, and concurrency
  assert!(!text.is_empty());
  println!("{}", text);
}
