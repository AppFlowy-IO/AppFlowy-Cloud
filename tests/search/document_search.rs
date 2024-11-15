use std::path::PathBuf;
use std::time::Duration;

use appflowy_ai_client::dto::CalculateSimilarityParams;
use client_api_test::{collect_answer, TestClient};
use collab::preclude::Collab;
use collab_document::document::Document;
use collab_document::importer::md_importer::MDImporter;
use collab_entity::CollabType;
use shared_entity::dto::chat_dto::{CreateChatMessageParams, CreateChatParams};
use tokio::time::sleep;
use workspace_template::document::getting_started::getting_started_document_data;

#[tokio::test]
async fn test_embedding_when_create_document() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let object_id_1 = uuid::Uuid::new_v4().to_string();
  let the_five_dysfunctions_of_a_team =
    create_document_collab(&object_id_1, "the_five_dysfunctions_of_a_team.md").await;
  let encoded_collab = the_five_dysfunctions_of_a_team.encode_collab().unwrap();
  test_client
    .create_collab_with_data(
      &workspace_id,
      &object_id_1,
      CollabType::Document,
      encoded_collab,
    )
    .await
    .unwrap();

  let object_id_2 = uuid::Uuid::new_v4().to_string();
  let tennis_player = create_document_collab(&object_id_2, "kathryn_tennis_story.md").await;
  let encoded_collab = tennis_player.encode_collab().unwrap();
  test_client
    .create_collab_with_data(
      &workspace_id,
      &object_id_2,
      CollabType::Document,
      encoded_collab,
    )
    .await
    .unwrap();
  let search_resp = test_client
    .api_client
    .search_documents(&workspace_id, "Kathryn", 5, 20)
    .await
    .unwrap();
  println!("{:?}", search_resp);

  // Create a chat to ask questions that related to the five dysfunctions of a team
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

  let params = CreateChatMessageParams::new_user("what kathryn did?");
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
    input: answer,
    expected: r#"
    ### Kathryn Petersen from *The Five Dysfunctions of a Team*
1. **New CEO Role**:
   - Appointed as the CEO of DecisionTech, a struggling startup with a dysfunctional executive team.

2. **Identifying Issues**:
   - Recognized that the team's problems stemmed from poor communication, lack of trust, and weak commitment.

3. **Building Trust**:
   - Organized an offsite meeting in Napa Valley to foster trust among team members through personal sharing and vulnerability.

4. **Encouraging Constructive Conflict**:
   - Promoted open discussions and healthy conflict to improve decision-making and team dynamics.

5. **Fostering Accountability**:
   - Held the team to high standards, emphasizing the importance of accountability and addressing issues directly.

6. **Focusing on Shared Goals**:
   - Reinforced commitment to collective achievements over individual successes, leading to improved team performance.

7. **Implementing a Model**:
   - Utilized a model that identified five key dysfunctions of a team and provided strategies to overcome them, focusing on trust, conflict, commitment, accountability, and results.
    "#.to_string(),
  };
  let score = test_client
    .api_client
    .calculate_similarity(params)
    .await
    .unwrap()
    .score;

  println!("{:?}", score);
}

#[ignore]
#[tokio::test]
async fn test_document_indexing_and_search() {
  // Set up all the required data
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = uuid::Uuid::new_v4().to_string();

  let collab_type = CollabType::Document;
  let encoded_collab = {
    let document_data = getting_started_document_data().unwrap();
    let collab = Collab::new(
      test_client.uid().await,
      object_id.clone(),
      test_client.device_id.clone(),
      vec![],
      false,
    );
    let document = Document::create_with_data(collab, document_data).unwrap();
    document.encode_collab().unwrap()
  };
  test_client
    .create_and_edit_collab_with_data(
      &object_id,
      &workspace_id,
      collab_type.clone(),
      Some(encoded_collab),
    )
    .await;
  test_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
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
