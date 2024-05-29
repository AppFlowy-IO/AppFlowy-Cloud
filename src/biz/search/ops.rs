use app_error::ErrorCode;
use database::index::{search_documents, SearchDocumentParams};
use openai_dive::v1::models::EmbeddingsEngine;
use openai_dive::v1::resources::embedding::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingOutput, EmbeddingParameters,
};
use shared_entity::dto::search_dto::{
  SearchContentType, SearchDocumentRequest, SearchDocumentResponseItem,
};
use shared_entity::response::AppResponseError;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn search_document(
  pg_pool: &PgPool,
  openai: &openai_dive::v1::api::Client,
  uid: i64,
  workspace_id: Uuid,
  request: SearchDocumentRequest,
) -> Result<Vec<SearchDocumentResponseItem>, AppResponseError> {
  let embeddings = openai
    .embeddings()
    .create(EmbeddingParameters {
      input: EmbeddingInput::String(request.query),
      model: EmbeddingsEngine::TextEmbedding3Small.to_string(),
      encoding_format: Some(EmbeddingEncodingFormat::Float),
      dimensions: Some(1536), // text-embedding-3-small default number of dimensions
      user: None,
    })
    .await
    .map_err(|e| AppResponseError::new(ErrorCode::Internal, e.to_string()))?;

  let tokens_used = if let Some(usage) = embeddings.usage {
    tracing::info!(
      "workspace {} OpenAI API search tokens used: {}",
      workspace_id,
      usage.total_tokens
    );
    usage.total_tokens
  } else {
    0
  };

  let embedding = embeddings
    .data
    .first()
    .ok_or_else(|| AppResponseError::new(ErrorCode::Internal, "OpenAI returned no embeddings"))?;
  let embedding = match &embedding.embedding {
    EmbeddingOutput::Float(vector) => vector.iter().map(|&v| v as f32).collect(),
    EmbeddingOutput::Base64(_) => {
      return Err(AppResponseError::new(
        ErrorCode::Internal,
        "OpenAI returned no embeddings in unsupported format",
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
      preview: request.preview_size.unwrap_or(180) as i32,
      embedding,
    },
    tokens_used,
  )
  .await?;
  tx.commit().await?;
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
