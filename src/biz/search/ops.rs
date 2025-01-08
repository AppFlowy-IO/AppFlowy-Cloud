use crate::api::metrics::RequestMetrics;
use crate::biz::collab::folder_view::{
  check_if_view_ancestors_fulfil_condition, private_and_nonviewable_view_ids,
};
use crate::biz::collab::utils::get_latest_collab_folder;
use app_error::ErrorCode;
use appflowy_ai_client::dto::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingModel, EmbeddingOutput, EmbeddingRequest,
};
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use database::collab::GetCollabOrigin;
use std::sync::Arc;

use database::index::{search_documents, SearchDocumentParams};
use shared_entity::dto::search_dto::{
  SearchContentType, SearchDocumentRequest, SearchDocumentResponseItem,
};
use shared_entity::response::AppResponseError;
use sqlx::PgPool;

use indexer::scheduler::IndexerScheduler;
use uuid::Uuid;

pub async fn search_document(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  indexer_scheduler: &Arc<IndexerScheduler>,
  uid: i64,
  workspace_id: Uuid,
  request: SearchDocumentRequest,
  metrics: &RequestMetrics,
) -> Result<Vec<SearchDocumentResponseItem>, AppResponseError> {
  let embeddings = indexer_scheduler
    .create_search_embeddings(EmbeddingRequest {
      input: EmbeddingInput::String(request.query.clone()),
      model: EmbeddingModel::TextEmbedding3Small.to_string(),
      encoding_format: EmbeddingEncodingFormat::Float,
      dimensions: EmbeddingModel::TextEmbedding3Small.default_dimensions(),
    })
    .await?;
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

  let folder = get_latest_collab_folder(
    collab_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
  )
  .await?;
  let private_and_nonviewable_views = private_and_nonviewable_view_ids(&folder);
  let non_searchable_view_ids = private_and_nonviewable_views.nonviewable_view_ids;
  let filtered_results = results.into_iter().filter(|item| {
    !check_if_view_ancestors_fulfil_condition(&item.object_id, &folder, |view| {
      non_searchable_view_ids.contains(&view.id)
    })
  });

  Ok(
    filtered_results
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
