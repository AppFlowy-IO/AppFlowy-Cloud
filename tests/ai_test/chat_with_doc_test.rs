use crate::collab::util::{alex_banker_story, alex_software_engineer_story, empty_document_editor};
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
  let mut editor = empty_document_editor(&object_id);
  let contents = alex_software_engineer_story();
  editor.insert_paragraphs(contents.into_iter().map(|s| s.to_string()).collect());
  let encode_collab = editor.encode_collab();

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
    .wait_until_get_embedding(&workspace_id, &object_id)
    .await;

  // chat with document
  let chat_id = uuid::Uuid::new_v4().to_string();
  let params = CreateChatParams {
    chat_id: chat_id.clone(),
    name: "my first chat".to_string(),
    rag_ids: vec![object_id.clone()],
  };

  // create a chat
  test_client
    .api_client
    .create_chat(&workspace_id, params)
    .await
    .unwrap();

  // ask question to check the chat is using document embedding or not
  let params = CreateChatMessageParams::new_user(
    "What are some of the sports Alex enjoys, and what are his experiences with them",
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
  Alex enjoys a variety of sports that keep him active and engaged:
	1.	Tennis: Learned in Singapore, he plays on weekends with friends.
	2.	Basketball: Enjoys casual play, though specific details aren’t provided.
	3.	Cycling: Brought his bike to Singapore and looks forward to exploring parks.
	4.	Badminton: Enjoys it, though details aren’t given.
	5.	Snowboarding: Had an unforgettable experience on challenging slopes in Lake Tahoe.
Overall, Alex balances his work as a software programmer with his passion for sports, finding excitement and freedom in each activity.
  "#;
  test_client
    .assert_similarity(&workspace_id, &answer, expected, 0.83)
    .await;

  // remove all content for given document
  editor.clear();

  // Simulate insert new content
  let contents = alex_banker_story();
  editor.insert_paragraphs(contents.into_iter().map(|s| s.to_string()).collect());
  let text = editor.document.to_plain_text(false, false).unwrap();
  let expected = alex_banker_story().join("");
  assert_eq!(text, expected);

  // full sync
  let encode_collab = editor.encode_collab();
  test_client
    .api_client
    .collab_full_sync(
      &workspace_id,
      &object_id,
      CollabType::Document,
      encode_collab.doc_state.to_vec(),
      encode_collab.state_vector.to_vec(),
    )
    .await
    .unwrap();

  // after full sync, chat with the same question. After update the document content, the chat
  // should not reply with previous context.
  let params = CreateChatMessageParams::new_user(
    "What are some of the sports Alex enjoys, and what are his experiences with them",
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
 Alex does not enjoy sports or physical activities. Instead, he prefers to relax and finds joy in
 exploring delicious food and trying new restaurants. For Alex, food is a form of relaxation and self-care,
 making it his favorite way to unwind rather than engaging in sports. While he may not have experiences with sports,
  he certainly has many experiences in the culinary world, where he enjoys savoring flavors and discovering new dishes
  "#;
  test_client
    .assert_similarity(&workspace_id, &answer, expected, 0.83)
    .await;
}
