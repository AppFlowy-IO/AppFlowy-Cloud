use crate::biz::collab::folder_view::PrivateSpaceAndTrashViews;
use crate::biz::collab::utils::get_latest_collab_folder;
use crate::{
  api::metrics::RequestMetrics, biz::collab::folder_view::private_space_and_trash_view_ids,
};
use app_error::AppError;
use appflowy_ai_client::dto::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingModel, EmbeddingOutput, EmbeddingRequest,
};
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use collab_folder::{Folder, View};
use database::collab::GetCollabOrigin;
use std::collections::HashSet;
use std::sync::Arc;

use database::index::{search_documents, SearchDocumentParams};
use shared_entity::dto::search_dto::{
  SearchContentType, SearchDocumentRequest, SearchDocumentResponseItem,
};
use sqlx::PgPool;

use indexer::scheduler::IndexerScheduler;
use uuid::Uuid;

static MAX_SEARCH_DEPTH: i32 = 10;

fn is_view_searchable(view: &View, workspace_id: &str) -> bool {
  view.id != workspace_id && view.parent_view_id != workspace_id && view.layout.is_document()
}

fn populate_searchable_view_ids(
  folder: &Folder,
  private_space_and_trash_views: &PrivateSpaceAndTrashViews,
  searchable_view_ids: &mut HashSet<Uuid>,
  workspace_id: &Uuid,
  current_view_id: &Uuid,
  depth: i32,
  max_depth: i32,
) {
  if depth > max_depth {
    return;
  }
  let is_other_private_space = private_space_and_trash_views
    .other_private_space_ids
    .contains(current_view_id);
  let is_trash = private_space_and_trash_views
    .view_ids_in_trash
    .contains(current_view_id);
  if is_other_private_space || is_trash {
    return;
  }
  let view = match folder.get_view(&current_view_id.to_string()) {
    Some(view) => view,
    None => return,
  };

  if is_view_searchable(&view, &workspace_id.to_string()) {
    searchable_view_ids.insert(*current_view_id);
  }
  for child in view.children.iter() {
    let child_id = Uuid::parse_str(&child.id).unwrap();
    populate_searchable_view_ids(
      folder,
      private_space_and_trash_views,
      searchable_view_ids,
      workspace_id,
      &child_id,
      depth + 1,
      max_depth,
    );
  }
}

pub async fn search_document(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  indexer_scheduler: &Arc<IndexerScheduler>,
  uid: i64,
  workspace_uuid: Uuid,
  request: SearchDocumentRequest,
  metrics: &RequestMetrics,
) -> Result<Vec<SearchDocumentResponseItem>, AppError> {
  let embeddings = indexer_scheduler
    .create_search_embeddings(EmbeddingRequest {
      input: EmbeddingInput::String(request.query.clone()),
      model: EmbeddingModel::TextEmbedding3Small.to_string(),
      encoding_format: EmbeddingEncodingFormat::Float,
      dimensions: EmbeddingModel::TextEmbedding3Small.default_dimensions(),
    })
    .await?;
  let total_tokens = embeddings.usage.total_tokens as u32;
  metrics.record_search_tokens_used(&workspace_uuid, total_tokens);
  tracing::info!(
    "workspace {} OpenAI API search tokens used: {}",
    workspace_uuid,
    total_tokens
  );

  let embedding = embeddings
    .data
    .first()
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OpenAI returned no embeddings")))?;
  let embedding = match &embedding.embedding {
    EmbeddingOutput::Float(vector) => vector.iter().map(|&v| v as f32).collect(),
    EmbeddingOutput::Base64(_) => {
      return Err(AppError::Internal(anyhow::anyhow!(
        "OpenAI returned embeddings in unsupported format"
      )))
    },
  };

  let folder = get_latest_collab_folder(
    collab_storage,
    GetCollabOrigin::User { uid },
    workspace_uuid,
  )
  .await?;
  let private_space_and_trash_views = private_space_and_trash_view_ids(&folder);
  let mut searchable_view_ids = HashSet::new();
  populate_searchable_view_ids(
    &folder,
    &private_space_and_trash_views,
    &mut searchable_view_ids,
    &workspace_uuid,
    &workspace_uuid,
    0,
    MAX_SEARCH_DEPTH,
  );
  let results = search_documents(
    pg_pool,
    SearchDocumentParams {
      user_id: uid,
      workspace_id: workspace_uuid,
      limit: request.limit.unwrap_or(10) as i32,
      preview: request.preview_size.unwrap_or(500) as i32,
      embedding,
      searchable_view_ids: searchable_view_ids.into_iter().collect(),
    },
    total_tokens,
  )
  .await?;
  tracing::trace!(
    "user {} search request in workspace {} returned {} results for query: `{}`",
    uid,
    workspace_uuid,
    results.len(),
    request.query
  );

  Ok(
    results
      .into_iter()
      .map(|item| SearchDocumentResponseItem {
        object_id: item.object_id,
        workspace_id: item.workspace_id,
        score: item.score,
        content_type: SearchContentType::from_record(item.content_type),
        preview: item.content_preview,
        created_by: item.created_by,
        created_at: item.created_at,
      })
      .collect(),
  )
}
