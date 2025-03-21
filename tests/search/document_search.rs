use std::path::PathBuf;
use std::time::Duration;

use appflowy_ai_client::dto::CalculateSimilarityParams;
use client_api_test::{collect_answer, TestClient};
use collab::preclude::Collab;
use collab_document::document::Document;
use collab_document::importer::md_importer::MDImporter;
use collab_entity::CollabType;
use collab_folder::ViewLayout;
use shared_entity::dto::chat_dto::{CreateChatMessageParams, CreateChatParams};
use tokio::time::sleep;
use workspace_template::document::getting_started::getting_started_document_data;

#[tokio::test]
async fn test_embedding_when_create_document() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id_1 = uuid::Uuid::new_v4();
  let the_five_dysfunctions_of_a_team = create_document_collab(
    &object_id_1.to_string(),
    "the_five_dysfunctions_of_a_team.md",
  )
  .await;
  let encoded_collab = the_five_dysfunctions_of_a_team.encode_collab().unwrap();
  test_client
    .create_collab_with_data(
      workspace_id,
      object_id_1,
      CollabType::Document,
      encoded_collab,
    )
    .await
    .unwrap();
  test_client
    .insert_view_to_general_space(
      &workspace_id,
      &object_id_1.to_string(),
      "five dysfunctional",
      ViewLayout::Document,
    )
    .await;

  test_client
    .wait_until_get_embedding(&workspace_id, &object_id_1)
    .await;

  let object_id_2 = uuid::Uuid::new_v4();
  let tennis_player =
    create_document_collab(&object_id_2.to_string(), "kathryn_tennis_story.md").await;
  let encoded_collab = tennis_player.encode_collab().unwrap();
  test_client
    .create_collab_with_data(
      workspace_id,
      object_id_2,
      CollabType::Document,
      encoded_collab,
    )
    .await
    .unwrap();
  test_client
    .insert_view_to_general_space(
      &workspace_id,
      &object_id_2.to_string(),
      "tennis",
      ViewLayout::Document,
    )
    .await;

  test_client
    .wait_until_get_embedding(&workspace_id, &object_id_2)
    .await;

  let search_resp = test_client
    .api_client
    .search_documents(&workspace_id, "Kathryn", 5, 100)
    .await
    .unwrap();
  // The number of returned documents affected by the max token size when splitting the document
  // into chunks.
  assert_eq!(search_resp.len(), 2);

  if ai_test_enabled() {
    let previews = search_resp
      .iter()
      .map(|item| item.preview.clone().unwrap())
      .collect::<Vec<String>>()
      .join("\n");
    let params = CalculateSimilarityParams {
      workspace_id,
      input: previews,
      expected: r#"
      "Kathryn’s Journey to Becoming a Tennis Player Kathryn’s love for tennis began on a warm summer day w
yn decided to pursue tennis seriously. She joined a competitive training academy, where the
practice
mwork. Part III: Heavy Lifting With initial trust in place, Kathryn shifts her focus to accountabili
’s ideas without fear of
reprisal. Lack of Commitment Without clarity and buy-in, team decisions bec
The Five Dysfunctions of a Team by Patrick Lencioni The Five Dysfunctions of a Team by Patrick Lenci"
    "#
      .to_string(),
      use_embedding: true,
    };
    let score = test_client
      .api_client
      .calculate_similarity(params)
      .await
      .unwrap()
      .score;

    assert!(
      score > 0.85,
      "preview score should greater than 0.85, but got: {}",
      score
    );

    // Create a chat to ask questions that related to the five dysfunctions of a team.
    let chat_id = uuid::Uuid::new_v4().to_string();
    let params = CreateChatParams {
      chat_id: chat_id.clone(),
      name: "chat with the five dysfunctions of a team".to_string(),
      rag_ids: vec![object_id_1],
    };

    test_client
      .api_client
      .create_chat(&workspace_id, params)
      .await
      .unwrap();

    let params = CreateChatMessageParams::new_user("Tell me what Kathryn concisely?");
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
    let answer = collect_answer(answer_stream).await;

    let params = CalculateSimilarityParams {
      workspace_id,
      input: answer.clone(),
      expected: r#"
    Kathryn Petersen is the newly appointed CEO of DecisionTech, a struggling Silicon Valley startup.
     She steps into a role facing a dysfunctional executive team characterized by poor communication,
      lack of trust, and weak commitment. Throughout the narrative, Kathryn focuses on addressing
      foundational team issues by fostering trust, encouraging open conflict, and promoting accountability,
       ultimately leading her team toward improved collaboration and performance.
    "#
          .to_string(),
      use_embedding: true,
    };
    let score = test_client
      .api_client
      .calculate_similarity(params)
      .await
      .unwrap()
      .score;

    assert!(
      score > 0.8,
      "expected: 0.8, but got score: {}, input:{}",
      score,
      answer
    );
  }
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
    let collab = Collab::new(
      test_client.uid().await,
      object_id.to_string(),
      test_client.device_id.clone(),
      vec![],
      false,
    );
    let document = Document::create_with_data(collab, document_data).unwrap();
    document.encode_collab().unwrap()
  };
  test_client
    .create_and_edit_collab_with_data(
      object_id,
      workspace_id,
      collab_type.clone(),
      Some(encoded_collab),
    )
    .await;
  test_client
    .open_collab(workspace_id, object_id, collab_type.clone())
    .await;

  sleep(Duration::from_millis(2000)).await;

  // document should get automatically indexed after opening if it wasn't indexed before
  let search_resp = test_client
    .api_client
    .search_documents(&workspace_id, "Appflowy", 1, 20)
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
  Document::create(document_id, document_data).unwrap()
}

pub fn ai_test_enabled() -> bool {
  if cfg!(feature = "ai-test-enabled") {
    return true;
  }
  false
}
