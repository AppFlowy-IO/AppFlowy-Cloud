use appflowy_cloud::api::search::create_ai_tool;
use client_api_test::{ai_test_enabled, load_env, setup_log};
use indexer::vector::embedder::get_open_ai_config;
use llm_client::chat::LLMDocument;
use uuid::Uuid;

#[tokio::test]
async fn chat_with_search_result_simple() {
  if !ai_test_enabled() {
    return;
  }
  let (open_ai_config, azure_config) = get_open_ai_config();
  let ai_chat = create_ai_tool(&azure_config, &open_ai_config).unwrap();
  let model_name = "gpt-4o-mini";
  let docs = vec![
    ("GPT-4o-mini is typically used to suggest a streamlined version of the GPT-4 family. The idea is to create a model that maintains the core language capabilities of GPT-4 while reducing computational requirements", Uuid::new_v4()),
    ("The name “Llama3.1” hints at an incremental evolution within the LLaMA (Large Language Model Meta AI) series—a family of models designed by Meta for research accessibility and performance efficiency",Uuid::new_v4()),
  ].into_iter().map(|(content, object_id)| LLMDocument::new(content.to_string(), object_id)).collect::<Vec<_>>();

  let resp = ai_chat
    .summarize_documents("gpt-4o", model_name, docs.clone(), true)
    .await
    .unwrap();
  dbg!(&resp);
  assert_eq!(resp.summaries.len(), 1);
  assert_eq!(resp.summaries[0].sources[0], docs[0].object_id);

  let resp = ai_chat
    .summarize_documents("deepseek-r1", model_name, docs.clone(), true)
    .await
    .unwrap();
  dbg!(&resp);
  assert_eq!(resp.summaries.len(), 0);

  // When only_context is false, the llm knowledge base is used to answer the question.
  let resp = ai_chat
    .summarize_documents("deepseek-r1 llm model", model_name, docs, false)
    .await
    .unwrap();
  dbg!(&resp);
  assert_eq!(resp.summaries.len(), 1);
  assert!(resp.summaries[0].sources.is_empty());
}

#[tokio::test]
async fn summary_search_result() {
  if !ai_test_enabled() {
    return;
  }
  load_env();
  setup_log();
  let (open_ai_config, azure_config) = get_open_ai_config();
  let ai_chat = create_ai_tool(&azure_config, &open_ai_config).unwrap();
  let model_name = "gpt-4o-mini";
  let docs = vec![
    ("Rust is a multiplayer survival game developed by Facepunch Studios, first released in early access in December 2013 and fully launched in February 2018. It has since become one of the most popular games in the survival genre, known for its harsh environment, intricate crafting system, and player-driven dynamics. The game is available on Windows, macOS, and PlayStation, with a community-driven approach to updates and content additions.", uuid::Uuid::new_v4()),
    ("Rust is a modern, system-level programming language designed with a focus on performance, safety, and concurrency. It was created by Mozilla and first released in 2010, with its 1.0 version launched in 2015. Rust is known for providing the control and performance of languages like C and C++, but with built-in safety features that prevent common programming errors, such as memory leaks, data races, and buffer overflows.", uuid::Uuid::new_v4()),
    ("Rust as a Natural Process (Oxidation) refers to the chemical reaction that occurs when metals, primarily iron, come into contact with oxygen and moisture (water) over time, leading to the formation of iron oxide, commonly known as rust. This process is a form of oxidation, where a substance reacts with oxygen in the air or water, resulting in the degradation of the metal.", uuid::Uuid::new_v4()),
  ].into_iter().map(|(content, object_id)| LLMDocument::new(content.to_string(), object_id)).collect::<Vec<_>>();

  let resp = ai_chat
    .summarize_documents("What is Rust", model_name, docs.clone(), true)
    .await
    .unwrap();
  dbg!(&resp);
  assert_eq!(resp.summaries.len(), 1);
  assert!(resp.summaries[0].sources.len() > 1);

  let resp = ai_chat
    .summarize_documents("multiplayer game", model_name, docs.clone(), true)
    .await
    .unwrap();
  dbg!(&resp);
  assert_eq!(resp.summaries.len(), 1);
  assert_eq!(resp.summaries[0].sources.len(), 1);
  assert_eq!(resp.summaries[0].sources[0], docs[0].object_id);
}

#[tokio::test]
async fn summary_search_result2() {
  if !ai_test_enabled() {
    return;
  }
  load_env();
  setup_log();
  let (open_ai_config, azure_config) = get_open_ai_config();
  let ai_chat = create_ai_tool(&azure_config, &open_ai_config).unwrap();
  let model_name = "gpt-4o-mini";
  let docs = vec![ LLMDocument::new("Kathryn leads exercises that reveal personal histories, enabling the team members to see each other beyond their\nprofessional roles. She also introduces the idea of constructive conflict, encouraging open discussion about\ndisagreements and differing opinions. Despite the discomfort this causes for some team members who are used to\nindividualistic work styles, Kathryn emphasizes that trust and openness are crucial for effective teamwork.Part III: Heavy LiftingWith initial trust in place, Kathryn shifts her focus to accountability and responsibility. This part highlights the\nchallenges team members face when taking ownership of collective goals.".to_string(), Uuid::new_v4())];

  let resp = ai_chat
    .summarize_documents("Kathryn tennis", model_name, docs.clone(), true)
    .await
    .unwrap();
  dbg!(&resp);
  assert_eq!(resp.summaries.len(), 1);
  assert!(resp.summaries[0].sources.len() > 1);
}
