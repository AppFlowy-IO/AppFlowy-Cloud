use crate::appflowy_ai_client;
use appflowy_ai_client::client::collect_stream_text;
use appflowy_ai_client::dto::{
  CompleteTextParams, CompletionMetadata, CompletionType, CustomPrompt, OutputContent,
  OutputLayout, ResponseFormat,
};

#[tokio::test]
async fn completion_explain_test() {
  let client = appflowy_ai_client();
  let params = CompleteTextParams {
    text: "Snowboarding".to_string(),
    completion_type: Some(CompletionType::Explain),
    metadata: Some(CompletionMetadata {
      object_id: uuid::Uuid::new_v4(),
      workspace_id: Some(uuid::Uuid::new_v4()),
      rag_ids: None,
      completion_history: None,
      custom_prompt: None,
      prompt_id: None,
    }),
    format: ResponseFormat::default(),
  };
  let stream = client
    .stream_completion_text(params, "gpt-4o-mini")
    .await
    .unwrap();
  let text = collect_stream_text(stream).await;
  assert!(!text.is_empty());
}

#[tokio::test]
async fn completion_image_test() {
  let client = appflowy_ai_client();
  let params = CompleteTextParams {
    text: "A yellow cat".to_string(),
    completion_type: Some(CompletionType::ImproveWriting),
    metadata: Some(CompletionMetadata {
      object_id: uuid::Uuid::new_v4(),
      workspace_id: Some(uuid::Uuid::new_v4()),
      rag_ids: None,
      completion_history: None,
      custom_prompt: None,
      prompt_id: None,
    }),
    format: ResponseFormat {
      output_content: OutputContent::IMAGE,
      ..Default::default()
    },
  };
  let stream = client
    .stream_completion_text(params, "gpt-4o-mini")
    .await
    .unwrap();
  let text = collect_stream_text(stream).await;
  println!("{}", text);
  assert!(text.contains("http://localhost"));
}

#[tokio::test]
async fn continue_writing_test() {
  let client = appflowy_ai_client();
  let params = CompleteTextParams {
    text: "I feel hungry".to_string(),
    completion_type: Some(CompletionType::ImproveWriting),
    metadata: None,
    format: ResponseFormat {
      output_layout: OutputLayout::SimpleTable,
      ..Default::default()
    },
  };
  let stream = client
    .stream_completion_text(params, "gpt-4o-mini")
    .await
    .unwrap();
  let text = collect_stream_text(stream).await;
  assert!(!text.is_empty());
  println!("{}", text);
}

#[tokio::test]
async fn make_text_shorter_text() {
  let client = appflowy_ai_client();
  let params = CompleteTextParams {
        text: "I have an immense passion and deep-seated affection for Rust, a modern, multi-paradigm, high-performance programming language that I find incredibly satisfying to use due to its focus on safety, speed, and concurrency".to_string(),
        completion_type: Some(CompletionType::MakeShorter),
        metadata: None,
        format: ResponseFormat::default(),
    };
  let stream = client
    .stream_completion_text(params, "gpt-4o-mini")
    .await
    .unwrap();

  let text = collect_stream_text(stream).await;

  // the response would be something like:
  // I'm deeply passionate about Rust, a modern, high-performance programming language, due to its emphasis on safety, speed, and concurrency
  assert!(!text.is_empty());
  println!("{}", text);
}

#[tokio::test]
async fn custom_prompt_test() {
  let client = appflowy_ai_client();
  let params = CompleteTextParams {
    text: "A yellow cat".to_string(),
    completion_type: Some(CompletionType::CustomPrompt),
    metadata: Some(CompletionMetadata {
      object_id: uuid::Uuid::new_v4(),
      workspace_id: Some(uuid::Uuid::new_v4()),
      rag_ids: None,
      completion_history: None,
      custom_prompt: Some(CustomPrompt {
        system: "You are a talented artist who excels at providing detailed, creative instructions on how to draw a picture".to_string(),
      }),
      prompt_id: None,
    }),
    format: Default::default(),
  };
  let stream = client
    .stream_completion_text(params, "gpt-4o-mini")
    .await
    .unwrap();
  let text = collect_stream_text(stream).await;
  println!("{}", text);
}
