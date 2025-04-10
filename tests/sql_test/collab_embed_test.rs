use crate::sql_test::util::{
  create_test_collab_document, create_test_user, select_all_fragments, setup_db, upsert_test_chunks,
};

use appflowy_ai_client::dto::EmbeddingModel;
use indexer::collab_indexer::split_text_into_chunks;
use sqlx::PgPool;

// Book content broken into logical chunks for testing
const TEST_CHUNKS: [&str; 6] = [
    "The Five Dysfunctions of a Team",
    "Part I: Underachievement - Introduces Kathryn Petersen, the newly appointed CEO of DecisionTech, a struggling Silicon Valley startup with a dysfunctional executive team.",
    "Part II: Lighting the Fire - Kathryn organizes an offsite meeting to build trust and introduce constructive conflict, encouraging open discussion about disagreements.",
    "Part IV: Traction - The team experiences benefits of improved trust and open conflict, with accountability becoming routine and meetings increasingly productive.",
    "The Model identifies five key dysfunctions: Absence of Trust, Fear of Conflict, Lack of Commitment, Avoidance of Accountability, and Inattention to Results.",
    "The book provides practical strategies for building trust, encouraging conflict, ensuring commitment, embracing accountability, and focusing on collective results."
];

#[sqlx::test(migrations = false)]
async fn insert_collab_embedding_fragment_test(pool: PgPool) {
  setup_db(&pool).await.unwrap();
  let mut paragraphs = TEST_CHUNKS
    .iter()
    .map(|&s| s.to_string())
    .collect::<Vec<_>>();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = create_test_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let doc_id = uuid::Uuid::new_v4();
  let workspace_id = user.workspace_id;
  create_test_collab_document(&pool, &user.uid, &workspace_id, &doc_id).await;

  let chunks_1 = split_text_into_chunks(
    doc_id,
    paragraphs.clone(),
    EmbeddingModel::TextEmbedding3Small,
    500,
    100,
  )
  .unwrap();

  upsert_test_chunks(&pool, &workspace_id, &doc_id, chunks_1.clone()).await;
  let fragments_1 = select_all_fragments(&pool, &doc_id).await;
  assert_eq!(chunks_1.len(), fragments_1.len());

  // simulate edit first paragraph.
  paragraphs[0].push_str(" by Patrick Lencioni is a compelling exploration of team dynamics and the common pitfalls that undermine successful collaboration.");
  let chunks_2 = split_text_into_chunks(
    doc_id,
    paragraphs.clone(),
    EmbeddingModel::TextEmbedding3Small,
    500,
    100,
  )
  .unwrap();

  // After edit, the first paragraph will be different. but the second paragraph will be the same.
  assert_ne!(chunks_1[0].fragment_id, chunks_2[0].fragment_id);
  assert_eq!(chunks_1[1].fragment_id, chunks_2[1].fragment_id);
  assert_eq!(chunks_2.len(), 2);

  // Simulate insert a new paragraph
  paragraphs.insert(3, "Part III: Heavy Lifting - Focuses on accountability and responsibility, with Kathryn holding the team to high standards and addressing issues directly.".to_string(),);
  let chunks_3 = split_text_into_chunks(
    doc_id,
    paragraphs.clone(),
    EmbeddingModel::TextEmbedding3Small,
    500,
    100,
  )
  .unwrap();

  // After insert a new paragraph, the second paragraph will be different, but the first will remain the same.
  assert_eq!(chunks_2[0].fragment_id, chunks_3[0].fragment_id);
  assert_ne!(chunks_2[1].fragment_id, chunks_3[1].fragment_id);
  assert_eq!(chunks_3.len(), 3);
}

#[sqlx::test(migrations = false)]
async fn test_embed_over_context_size(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = create_test_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let doc_id = uuid::Uuid::new_v4();
  let workspace_id = user.workspace_id;
  create_test_collab_document(&pool, &user.uid, &workspace_id, &doc_id).await;
  let content= "The Five Dysfunctions of a Team Part I: Underachievement - Introduces Kathryn Petersen, the newly appointed CEO of DecisionTech, a struggling Silicon Valley startup with a dysfunctional executive team. Part II: Lighting the Fire - Kathryn organizes an offsite meeting to build trust and introduce constructive conflict, encouraging open discussion about disagreements. Part IV: Traction - The team experiences benefits of improved trust and open conflict, with accountability becoming routine and meetings increasingly productive. The Model identifies five key dysfunctions: Absence of Trust, Fear of Conflict, Lack of Commitment, Avoidance of Accountability, and Inattention to Results. The book provides practical strategies for building trust, encouraging conflict, ensuring commitment, embracing accountability, and focusing on collective results.";
  let chunks = split_text_into_chunks(
    doc_id,
    vec![content.to_string()],
    EmbeddingModel::TextEmbedding3Small,
    300,
    100,
  )
  .unwrap();

  assert_eq!(chunks.len(), 5);
  upsert_test_chunks(&pool, &workspace_id, &doc_id, chunks.clone()).await;
  let fragments = select_all_fragments(&pool, &doc_id).await;
  assert_eq!(chunks.len(), fragments.len());

  // Replace the content with a new one. It will cause all existing fragments to be deleted.
  let content = "Hello world!";
  let chunks = split_text_into_chunks(
    doc_id,
    vec![content.to_string()],
    EmbeddingModel::TextEmbedding3Small,
    300,
    100,
  )
  .unwrap();
  upsert_test_chunks(&pool, &workspace_id, &doc_id, chunks.clone()).await;
  let fragments = select_all_fragments(&pool, &doc_id).await;
  assert_eq!(fragments.len(), 1);
}
