use std::path::PathBuf;
use std::time::Duration;

use appflowy_ai_client::dto::CalculateSimilarityParams;
use client_api_test::{ai_test_enabled, collect_answer, TestClient};
use collab::core::collab::{default_client_id, CollabOptions};
use collab::core::origin::{CollabClient, CollabOrigin};
use collab::preclude::Collab;
use collab_document::document::Document;
use collab_document::importer::md_importer::MDImporter;
use collab_entity::CollabType;
use collab_folder::ViewLayout;
use shared_entity::dto::chat_dto::{CreateChatMessageParams, CreateChatParams};
use shared_entity::dto::search_dto::SearchResult;
use tokio::time::sleep;
use uuid::Uuid;
use workspace_template::document::getting_started::getting_started_document_data;

#[tokio::test]
#[ignore]
async fn test_embedding_when_create_document() {
  if !ai_test_enabled() {
    return;
  }

  let mut test_client = TestClient::new_user().await;
  let uid = test_client.uid().await;
  let workspace_id = test_client.workspace_id().await;
  // Create the first document and wait for its embedding.
  let object_id_1 = add_document_collab(
    &mut test_client,
    &workspace_id,
    "the_five_dysfunctions_of_a_team.md",
    "five dysfunctional",
    true,
    uid,
  )
  .await;

  // Test Search
  let query = "Overcoming the Five Dysfunctions";
  let items = test_client
    .wait_unit_get_search_result(&workspace_id, query, 5, 100, Some(0.2))
    .await
    .unwrap();
  dbg!("search result: {:?}", &items);

  // Test search summary
  let result = test_client
    .api_client
    .generate_search_summary(
      &workspace_id,
      query,
      items.iter().map(SearchResult::from).collect(),
    )
    .await
    .unwrap();
  dbg!("search summary: {}", &result);
  assert!(!result.summaries.is_empty());

  let previews = items
    .iter()
    .map(|item| item.preview.clone().unwrap())
    .collect::<Vec<String>>()
    .join("\n");
  let expected = "The Five Dysfunctions of a Team illustrates strategies for overcoming common organizational challenges by fostering trust, accountability, and shared goals to improve team dynamics.";
  calculate_similarity_and_assert(
    &mut test_client,
    workspace_id,
    previews,
    expected,
    0.7,
    "preview score",
  )
  .await;

  // Test irrelevant search
  let query = "Hello world";
  let items = test_client
    .api_client
    .search_documents(&workspace_id, query, 5, 100, Some(0.4))
    .await
    .unwrap();
  assert!(items.is_empty());

  let result = test_client
    .api_client
    .generate_search_summary(
      &workspace_id,
      query,
      items.into_iter().map(|v| SearchResult::from(&v)).collect(),
    )
    .await
    .unwrap();
  assert!(result.summaries.is_empty());

  // Simulate when user click search result to open the document and then chat with it.
  let answer = create_chat_and_ask_question(
    &mut test_client,
    &workspace_id,
    object_id_1,
    "chat with the five dysfunctions of a team",
    "Kathryn CEO of DecisionTech",
  )
  .await;

  let expected_answer = r#"
Kathryn Petersen is the newly appointed CEO of DecisionTech, a struggling Silicon Valley startup featured in Patrick Lencioni's book "The Five Dysfunctions of a Team." She faces the challenge of leading a dysfunctional executive team characterized by poor communication, lack of trust, and weak commitment. Her role involves addressing these issues to improve team dynamics and overall performance within the company.
  "#;

  calculate_similarity_and_assert(
    &mut test_client,
    workspace_id,
    answer.clone(),
    expected_answer,
    0.7,
    "expected",
  )
  .await;
}

#[ignore]
#[tokio::test]
async fn test_document_indexing_and_search() {
  // Set up all the required data
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = uuid::Uuid::new_v4();

  let collab_type = CollabType::Document;
  let encoded_collab = {
    let document_data = getting_started_document_data().unwrap();
    let options = CollabOptions::new(object_id.to_string(), default_client_id());
    let collab = Collab::new_with_options(
      CollabOrigin::Client(CollabClient::new(
        test_client.uid().await,
        test_client.device_id.clone(),
      )),
      options,
    )
    .unwrap();
    let document = Document::create_with_data(collab, document_data).unwrap();
    document.encode_collab().unwrap()
  };
  test_client
    .create_and_edit_collab_with_data(
      object_id,
      workspace_id,
      collab_type,
      Some(encoded_collab),
      true,
    )
    .await;
  test_client
    .open_collab(workspace_id, object_id, collab_type)
    .await;

  sleep(Duration::from_millis(2000)).await;

  // document should get automatically indexed after opening if it wasn't indexed before
  let search_resp = test_client
    .api_client
    .search_documents(&workspace_id, "Appflowy", 1, 20, None)
    .await
    .unwrap();
  assert_eq!(search_resp.len(), 1);
  let item = &search_resp[0];
  assert_eq!(item.object_id, object_id);

  let preview = item.preview.clone().unwrap();
  assert!(preview.contains("Welcome to AppFlowy"));
}

async fn create_document_collab(document_id: &str, file_name: &str) -> Document {
  let file_path = PathBuf::from(format!("tests/search/asset/{}", file_name));
  let md = std::fs::read_to_string(file_path).unwrap();
  let importer = MDImporter::new(None);
  let document_data = importer.import(document_id, md).unwrap();
  Document::create(document_id, document_data, default_client_id()).unwrap()
}

async fn add_document_collab(
  client: &mut TestClient,
  workspace_id: &Uuid,
  file_name: &str,
  search_term: &str,
  wait_embedding: bool,
  uid: i64,
) -> Uuid {
  let object_id = Uuid::new_v4();
  let collab = create_document_collab(&object_id.to_string(), file_name).await;
  println!("create document with content: {:?}", collab.paragraphs());
  let encoded = collab.encode_collab().unwrap();
  client
    .create_collab_with_data(*workspace_id, object_id, CollabType::Document, encoded)
    .await
    .unwrap();
  client
    .insert_view_to_general_space(
      workspace_id,
      &object_id.to_string(),
      search_term,
      ViewLayout::Document,
      uid,
    )
    .await;
  if wait_embedding {
    client
      .wait_until_get_embedding(workspace_id, &object_id)
      .await
      .unwrap();
  }
  object_id
}

async fn create_chat_and_ask_question(
  test_client: &mut TestClient,
  workspace_id: &Uuid,
  rag_id: uuid::Uuid,
  chat_name: &str,
  question: &str,
) -> String {
  // Create a chat
  let chat_id = uuid::Uuid::new_v4().to_string();
  let params = CreateChatParams {
    chat_id: chat_id.clone(),
    name: chat_name.to_string(),
    rag_ids: vec![rag_id],
  };

  test_client
    .api_client
    .create_chat(workspace_id, params)
    .await
    .unwrap();

  // Ask question and get answer
  let params = CreateChatMessageParams::new_user(question);
  let question = test_client
    .api_client
    .create_question(workspace_id, &chat_id, params)
    .await
    .unwrap();
  let answer_stream = test_client
    .api_client
    .stream_answer_v2(workspace_id, &chat_id, question.message_id)
    .await
    .unwrap();
  collect_answer(answer_stream).await
}

async fn calculate_similarity_and_assert(
  test_client: &mut TestClient,
  workspace_id: Uuid,
  input: String,
  expected: &str,
  threshold: f64,
  error_message: &str,
) -> f64 {
  let params = CalculateSimilarityParams {
    workspace_id,
    input: input.clone(),
    expected: expected.to_string(),
    use_embedding: true,
  };

  let score = test_client
    .api_client
    .calculate_similarity(params)
    .await
    .unwrap()
    .score;

  assert!(
    score > threshold,
    "{} should greater than {}, but got: {}. input:{}, expected: {}",
    error_message,
    threshold,
    score,
    input,
    expected
  );

  score
}
