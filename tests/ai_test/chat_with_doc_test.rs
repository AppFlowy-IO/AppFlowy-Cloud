use crate::collab::util::nathan_document_encode_collab;
use appflowy_ai_client::dto::CalculateSimilarityParams;
use client_api_test::{ai_test_enabled, collect_answer, TestClient};
use collab_entity::CollabType;
use database_entity::dto::CreateCollabParams;
use shared_entity::dto::chat_dto::{CreateChatMessageParams, CreateChatParams};
use uuid::Uuid;

#[tokio::test]
async fn query_collab_embedding_after_create_test() {
  if !ai_test_enabled() {
    return;
  }

  let object_id = Uuid::new_v4().to_string();
  let encode_collab = nathan_document_encode_collab(&object_id);

  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let params = CreateCollabParams {
    workspace_id: workspace_id.clone(),
    object_id: object_id.clone(),
    encoded_collab_v1: encode_collab.encode_to_bytes().unwrap(),
    collab_type: CollabType::Document,
  };
  test_client.api_client.create_collab(params).await.unwrap();
  test_client
    .wait_until_object_embedding(&workspace_id, &object_id)
    .await;

  // chat with document
  let chat_id = uuid::Uuid::new_v4().to_string();
  let params = CreateChatParams {
    chat_id: chat_id.clone(),
    name: "my first chat".to_string(),
    rag_ids: vec![object_id],
  };

  test_client
    .api_client
    .create_chat(&workspace_id, params)
    .await
    .unwrap();

  let params = CreateChatMessageParams::new_user(
    "What are some of the sports Nathan enjoys, and what are his experiences with them",
  );
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

  let expected = r#"
### Sports Nathan Enjoys:
1. **Tennis**
   - **Experience**: Nathan learned tennis while living in Singapore. He enjoys playing with friends
     on weekends, which offers him both a social and competitive outlet.

2. **Basketball**
   - **Experience**: While specific experiences with basketball are not detailed, it is one of the
     sports he enjoys, likely providing him with team play and excitement.

3. **Cycling**
   - **Experience**: Nathan brought his bike with him when he moved to Singapore, eager to explore
     the city's parks and scenic routes. Although he hasn't had the chance to ride in Singapore yet,
     he looks forward to it.

4. **Badminton**
   - **Experience**: Like basketball, specific experiences in badminton are not mentioned, but it is
     another sport Nathan enjoys, indicating his love for racquet sports.

5. **Snowboarding**
   - **Experience**: Nathan had an unforgettable experience trying two diamond slopes in Lake Tahoe,
     which challenged his snowboarding skills. This indicates a willingness to take on challenges
     and enjoy the thrill of the sport.

### Overview of Nathan's Approach to Sports:
Nathan finds a balance between his work as a software programmer and his physical activities. He
enjoys the adrenaline from snowboarding and the strategic nature of tennis, which together provide
him with a sense of freedom and excitement in his life
"#;

  let params = CalculateSimilarityParams {
    workspace_id: workspace_id.clone(),
    input: answer,
    expected: expected.to_string(),
  };

  let resp = test_client
    .api_client
    .calculate_similarity(params)
    .await
    .unwrap();

  assert!(
    resp.score > 0.8,
    "Similarity score is too low: {}",
    resp.score
  );
}
