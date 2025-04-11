use chrono::{DateTime, Utc};
use pgvector::Vector;
use sqlx::{Executor, Postgres};
use tracing::trace;
use uuid::Uuid;

/// Logs each search request to track usage by workspace. It either inserts a new record or updates
/// an existing one with the current date, workspace ID, request count, and token usage. This ensures
/// accurate usage tracking for billing or monitoring.
///
/// Searches and retrieves documents based on their similarity to a given search embedding.
/// It filters by workspace, user access, and document status, and returns a limited number
/// of the most relevant documents, sorted by similarity score.
pub async fn search_documents<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  params: SearchDocumentParams,
  tokens_used: u32,
) -> Result<Vec<SearchDocumentResult>, sqlx::Error> {
  trace!(
    "search documents: user_id: {}, workspace_id: {}, limit: {}, score limit: {:?}",
    params.user_id,
    params.workspace_id,
    params.limit,
    params.score,
  );

  let query = sqlx::query_as::<_, SearchDocumentRow>(
    r#"
    WITH workspace AS (
      INSERT INTO af_workspace_ai_usage(created_at, workspace_id, search_requests, search_tokens_consumed, index_tokens_consumed)
      VALUES (now()::date, $2, 1, $6, 0)
      ON CONFLICT (created_at, workspace_id) DO UPDATE
      SET search_requests = af_workspace_ai_usage.search_requests + 1,
          search_tokens_consumed = af_workspace_ai_usage.search_tokens_consumed + $6
      RETURNING workspace_id
    )
    SELECT
      em.oid AS object_id,
      collab.workspace_id,
      collab.partition_key AS collab_type,
      em.content_type,
      em.content AS content,
      LEFT(em.content, $4) AS content_preview,
      u.name AS created_by,
      collab.created_at AS created_at,
      em.embedding <=> $3 AS distance
    FROM af_collab_embeddings em
    JOIN af_collab collab ON em.oid = collab.oid
    JOIN af_user u ON collab.owner_uid = u.uid
    WHERE collab.workspace_id = $2 AND (collab.oid = ANY($7::uuid[]))
    ORDER BY em.embedding <=> $3
    LIMIT $5
  "#,
  )
  .bind(params.user_id)
  .bind(params.workspace_id)
  .bind(Vector::from(params.embedding))
  .bind(params.preview)
  .bind(params.limit)
  .bind(tokens_used as i64)
  .bind(params.searchable_view_ids);
  let rows = query.fetch_all(executor).await?;
  let results = rows
    .into_iter()
    .map(|result| SearchDocumentResult {
      object_id: result.object_id,
      workspace_id: result.workspace_id,
      collab_type: result.collab_type,
      content_type: result.content_type,
      content: result.content,
      created_by: result.created_by,
      created_at: result.created_at,
      score: _cosine_relevance_score_fn(result.distance),
    })
    .collect::<Vec<_>>();

  trace!(
    "search documents: found {} results, scores: {:?}",
    results.len(),
    results.iter().map(|r| r.score).collect::<Vec<_>>()
  );

  let filter_result = results
    .into_iter()
    .filter(|result| result.score > params.score)
    .collect::<Vec<_>>();

  Ok(filter_result)
}

fn _cosine_relevance_score_fn(distance: f64) -> f64 {
  1.0 - distance
}

#[derive(Debug, Clone)]
pub struct SearchDocumentParams {
  /// ID of the user who is searching.
  pub user_id: i64,
  /// Workspace ID to search for documents in.
  pub workspace_id: Uuid,
  /// How many results should be returned.
  pub limit: i32,
  /// How many characters of the content (starting from the beginning) should be returned.
  pub preview: i32,
  /// Embedding of the query - generated by OpenAI embedder.
  pub embedding: Vec<f32>,
  /// List of view ids which is not supposed to be returned in the search results.
  pub searchable_view_ids: Vec<Uuid>,
  /// similarity score limit for the search results. The higher, the better.
  pub score: f64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SearchDocumentRow {
  /// Document identifier.
  pub object_id: Uuid,
  /// Workspace identifier, given document belongs to.
  pub workspace_id: Uuid,
  /// Partition key, which maps directly onto [collab_entity::CollabType].
  pub collab_type: i32,
  /// Type of the content to be presented. Maps directly onto [database_entity::dto::EmbeddingContentType].
  pub content_type: i32,
  /// Content of the document.
  pub content: String,
  /// Name of the user who's an owner of the document.
  pub created_by: String,
  /// When the document was created.
  pub created_at: DateTime<Utc>,
  /// Similarity score to an original query. Lower is better.
  pub distance: f64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SearchDocumentResult {
  /// Document identifier.
  pub object_id: Uuid,
  /// Workspace identifier, given document belongs to.
  pub workspace_id: Uuid,
  /// Partition key, which maps directly onto [collab_entity::CollabType].
  pub collab_type: i32,
  /// Type of the content to be presented. Maps directly onto [database_entity::dto::EmbeddingContentType].
  pub content_type: i32,
  /// Content of the document.
  pub content: String,
  /// Name of the user who's an owner of the document.
  pub created_by: String,
  /// When the document was created.
  pub created_at: DateTime<Utc>,
  /// Similarity score to an original query. Higher is better.
  pub score: f64,
}
