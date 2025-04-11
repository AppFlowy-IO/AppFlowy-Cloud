use appflowy_cloud::api::search::create_ai_chat_client;
use client_api_test::{ai_test_enabled, load_env};
use indexer::vector::embedder::get_open_ai_config;
use llm_client::chat::LLMDocument;
use serde_json::json;

#[tokio::test]
async fn chat_with_search_result_simple() {
  if !ai_test_enabled() {
    return;
  }
  load_env();
  let (open_ai_config, azure_config) = get_open_ai_config();
  let ai_chat = create_ai_chat_client(&azure_config, &open_ai_config).unwrap();
  let model_name = "gpt-4o-mini";
  let docs = vec![
    "“GPT-4o-mini” is typically used to suggest a streamlined version of the GPT-4 family. The idea is to create a model that maintains the core language capabilities of GPT-4 while reducing computational requirements",
    "The name “Llama3.1” hints at an incremental evolution within the LLaMA (Large Language Model Meta AI) series—a family of models designed by Meta for research accessibility and performance efficiency",
  ].into_iter().map(|content| LLMDocument::new(content.to_string(), json!({
    "id": content.len().to_string(),
    "source": if content.contains("GPT-4") { "openai_docs" } else { "meta_docs" },
    "timestamp": if content.contains("GPT-4") { "2024-01-01" } else { "2024-02-01" }
  }))).collect::<Vec<_>>();

  let resp = ai_chat
    .chat_with_documents("gpt-4o", model_name, &docs, true)
    .await
    .unwrap();
  dbg!(&resp);
  assert_eq!(resp.summaries.len(), 1);
  assert!(resp.summaries[0].score.parse::<f32>().unwrap() > 0.6);
  assert_eq!(
    resp.summaries[0].metadata.get("source").unwrap(),
    "openai_docs"
  );
  assert_eq!(
    resp.summaries[0].metadata.get("timestamp").unwrap(),
    "2024-01-01"
  );

  let resp = ai_chat
    .chat_with_documents("deepseek-r1", model_name, &docs, true)
    .await
    .unwrap();
  dbg!(&resp);
  assert_eq!(resp.summaries.len(), 0);

  // When only_context is false, the llm knowledge base is used to answer the question.
  let resp = ai_chat
    .chat_with_documents("deepseek-r1 llm model", model_name, &docs, false)
    .await
    .unwrap();
  dbg!(&resp);
  assert_eq!(resp.summaries.len(), 1);
  assert!(resp.summaries[0].score.parse::<f32>().unwrap() > 0.6);
  assert_eq!(resp.summaries[0].metadata, json!({}));
}

#[tokio::test]
async fn summary_search_result() {
  if !ai_test_enabled() {
    return;
  }
  load_env();
  let (open_ai_config, azure_config) = get_open_ai_config();
  let ai_chat = create_ai_chat_client(&azure_config, &open_ai_config).unwrap();
  let model_name = "gpt-4o-mini";
  let docs = vec![
    ("Rust is a multiplayer survival game developed by Facepunch Studios, first released in early access in December 2013 and fully launched in February 2018. It has since become one of the most popular games in the survival genre, known for its harsh environment, intricate crafting system, and player-driven dynamics. The game is available on Windows, macOS, and PlayStation, with a community-driven approach to updates and content additions.", json!({"id": 1, "source": "test"})),
    ("Rust is a modern, system-level programming language designed with a focus on performance, safety, and concurrency. It was created by Mozilla and first released in 2010, with its 1.0 version launched in 2015. Rust is known for providing the control and performance of languages like C and C++, but with built-in safety features that prevent common programming errors, such as memory leaks, data races, and buffer overflows.", json!({"id": 2, "source": "test2"})),
    ("Rust as a Natural Process (Oxidation) refers to the chemical reaction that occurs when metals, primarily iron, come into contact with oxygen and moisture (water) over time, leading to the formation of iron oxide, commonly known as rust. This process is a form of oxidation, where a substance reacts with oxygen in the air or water, resulting in the degradation of the metal.", json!({"id": 3})),
  ].into_iter().map(|(content, metadata)| LLMDocument::new(content.to_string(), metadata)).collect::<Vec<_>>();

  let resp = ai_chat
    .chat_with_documents("Rust", model_name, &docs, true)
    .await
    .unwrap();
  dbg!(&resp);
  assert_eq!(resp.summaries.len(), 3);

  let resp = ai_chat
    .chat_with_documents("Play Rust over time", model_name, &docs, true)
    .await
    .unwrap();
  dbg!(&resp);
  assert_eq!(resp.summaries.len(), 1);
}
