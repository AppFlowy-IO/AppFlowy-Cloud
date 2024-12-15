use crate::api::metrics::RequestMetrics;
use app_error::ErrorCode;
use appflowy_ai_client::client::AppFlowyAIClient;
use appflowy_ai_client::dto::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingModel, EmbeddingOutput, EmbeddingRequest,
};

use database::index::{search_documents, SearchDocumentParams};
use shared_entity::dto::search_dto::{
  SearchContentType, SearchDocumentRequest, SearchDocumentResponseItem,
};
use shared_entity::response::AppResponseError;
use sqlx::PgPool;

use uuid::Uuid;

pub async fn search_document(
  pg_pool: &PgPool,
  ai_client: &AppFlowyAIClient,
  uid: i64,
  workspace_id: Uuid,
  request: SearchDocumentRequest,
  metrics: &RequestMetrics,
) -> Result<Vec<SearchDocumentResponseItem>, AppResponseError> {
  let embeddings = ai_client
    .embeddings(EmbeddingRequest {
      input: EmbeddingInput::String(request.query.clone()),
      model: EmbeddingModel::TextEmbedding3Small.to_string(),
      encoding_format: EmbeddingEncodingFormat::Float,
      dimensions: EmbeddingModel::TextEmbedding3Small.default_dimensions(),
    })
    .map_err(|e| AppResponseError::new(ErrorCode::Internal, e.to_string()))?;
  let total_tokens = embeddings.usage.total_tokens as u32;
  metrics.record_search_tokens_used(&workspace_id, total_tokens);
  tracing::info!(
    "workspace {} OpenAI API search tokens used: {}",
    workspace_id,
    total_tokens
  );

  let embedding = embeddings
    .data
    .first()
    .ok_or_else(|| AppResponseError::new(ErrorCode::Internal, "OpenAI returned no embeddings"))?;
  let embedding = match &embedding.embedding {
    EmbeddingOutput::Float(vector) => vector.iter().map(|&v| v as f32).collect(),
    EmbeddingOutput::Base64(_) => {
      return Err(AppResponseError::new(
        ErrorCode::Internal,
        "OpenAI returned embeddings in unsupported format",
      ))
    },
  };

  let mut tx = pg_pool
    .begin()
    .await
    .map_err(|e| AppResponseError::new(ErrorCode::Internal, e.to_string()))?;
  let results = search_documents(
    &mut tx,
    SearchDocumentParams {
      user_id: uid,
      workspace_id,
      limit: request.limit.unwrap_or(10) as i32,
      preview: request.preview_size.unwrap_or(500) as i32,
      embedding,
    },
    total_tokens,
  )
  .await?;
  tx.commit().await?;
  tracing::trace!(
    "user {} search request in workspace {} returned {} results for query: `{}`",
    uid,
    workspace_id,
    results.len(),
    request.query
  );
  Ok(
    results
      .into_iter()
      .map(|item| SearchDocumentResponseItem {
        object_id: item.object_id,
        workspace_id: item.workspace_id.to_string(),
        score: item.score,
        content_type: SearchContentType::from_record(item.content_type),
        preview: item.content_preview,
        created_by: item.created_by,
        created_at: item.created_at,
      })
      .collect(),
  )
}
