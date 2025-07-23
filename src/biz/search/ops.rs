use crate::biz::collab::folder_view::PrivateSpaceAndTrashViews;
use crate::{
  api::metrics::RequestMetrics, biz::collab::folder_view::private_space_and_trash_view_ids,
};
use app_error::AppError;
use appflowy_ai_client::dto::EmbeddingModel;
use appflowy_collaborate::ws2::WorkspaceCollabInstanceCache;
use collab_folder::{Folder, View};
use database::index::{search_documents, SearchDocumentParams};
use indexer::scheduler::IndexerScheduler;
use indexer::vector::embedder::{CreateEmbeddingRequestArgs, EmbeddingInput, EncodingFormat};
use infra::env_util::get_env_var;
use llm_client::chat::{AITool, LLMDocument};
use shared_entity::dto::search_dto::{
  SearchContentType, SearchDocumentRequest, SearchDocumentResponseItem, SearchSummaryResult,
  Summary, SummarySearchResultRequest,
};
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{error, trace};
use uuid::Uuid;

static MAX_SEARCH_DEPTH: i32 = 10;

fn is_view_searchable(view: &View, workspace_id: &str) -> bool {
  view.id != workspace_id && view.parent_view_id != workspace_id && view.layout.is_document()
}

#[allow(clippy::too_many_arguments)]
fn populate_searchable_view_ids(
  folder: &Folder,
  private_space_and_trash_views: &PrivateSpaceAndTrashViews,
  searchable_view_ids: &mut HashSet<Uuid>,
  workspace_id: &Uuid,
  current_view_id: &Uuid,
  depth: i32,
  max_depth: i32,
  uid: i64,
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
  let view = match folder.get_view(&current_view_id.to_string(), uid) {
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
      uid,
    );
  }
}

#[allow(clippy::too_many_arguments)]
pub async fn search_document(
  pg_pool: &PgPool,
  collab_instance_cache: &impl WorkspaceCollabInstanceCache,
  indexer_scheduler: &Arc<IndexerScheduler>,
  uid: i64,
  workspace_uuid: Uuid,
  request: SearchDocumentRequest,
  metrics: &RequestMetrics,
) -> Result<Vec<SearchDocumentResponseItem>, AppError> {
  // Set up the embedding model and create an embedding request.
  let default_model = EmbeddingModel::default_model();
  let embeddings_request = CreateEmbeddingRequestArgs::default()
    .model(default_model.to_string())
    .input(EmbeddingInput::String(request.query.clone()))
    .encoding_format(EncodingFormat::Float)
    .dimensions(default_model.default_dimensions())
    .build()
    .map_err(|err| AppError::Unhandled(err.to_string()))?;

  // Create embeddings using the indexer scheduler.
  let mut embeddings_resp = indexer_scheduler
    .create_search_embeddings(embeddings_request)
    .await?;
  let total_tokens = embeddings_resp.usage.total_tokens;
  metrics.record_search_tokens_used(&workspace_uuid, total_tokens);
  tracing::info!(
    "workspace {} OpenAI API search tokens used: {}",
    workspace_uuid,
    total_tokens
  );

  // Extract the embedding from the response.
  let embedding = embeddings_resp
    .data
    .pop()
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OpenAI returned no embeddings")))?;

  // Obtain the latest collab folder and gather searchable view IDs.
  let folder = collab_instance_cache.get_folder(workspace_uuid).await?;
  let private_views = private_space_and_trash_view_ids(uid, &folder)?;
  let mut searchable_view_ids = HashSet::new();
  populate_searchable_view_ids(
    &folder,
    &private_views,
    &mut searchable_view_ids,
    &workspace_uuid,
    &workspace_uuid,
    0,
    MAX_SEARCH_DEPTH,
    uid,
  );

  // Set default preview size and search parameters.
  let preview_size = request.preview_size.unwrap_or(500) as i32;
  let params = SearchDocumentParams {
    user_id: uid,
    workspace_id: workspace_uuid,
    limit: request.limit.unwrap_or(10) as i32,
    preview: preview_size,
    embedding: embedding.embedding,
    searchable_view_ids: searchable_view_ids.into_iter().collect(),
    score: request.score,
  };

  trace!(
    "[Search] query: {}, limit: {}, score: {:?}, workspace: {}",
    request.query,
    params.limit,
    params.score,
    params.workspace_id,
  );

  // Perform document search.
  let results = search_documents(pg_pool, params, total_tokens).await?;
  trace!(
    "[Search] query:{}, got {} results",
    request.query,
    results.len(),
  );

  // Build and return the search result, mapping each document to its response item.
  let items = results
    .into_iter()
    .map(|item| SearchDocumentResponseItem {
      object_id: item.object_id,
      workspace_id: item.workspace_id,
      score: item.score,
      content_type: SearchContentType::from_record(item.content_type),
      preview: Some(item.content.chars().take(preview_size as usize).collect()),
      created_by: item.created_by,
      created_at: item.created_at,
      content: item.content,
    })
    .collect();

  Ok(items)
}

pub async fn summarize_search_results(
  ai_tool: Option<AITool>,
  request: SummarySearchResultRequest,
) -> Result<SearchSummaryResult, AppError> {
  if request.search_results.is_empty() {
    return Ok(SearchSummaryResult { summaries: vec![] });
  }

  if ai_tool.is_none() {
    return Err(AppError::FeatureNotAvailable(
      "AI tool is not available".to_string(),
    ));
  }

  let ai_tool = ai_tool.unwrap();
  let model_name = get_env_var("AI_OPENAI_API_SUMMARY_MODEL", "gpt-4.1-nano");

  let mut summaries = Vec::new();
  let SummarySearchResultRequest {
    query,
    search_results,
    only_context,
  } = request;

  let llm_docs: Vec<LLMDocument> = search_results
    .into_iter()
    .map(|result| LLMDocument::new(result.content, result.object_id))
    .collect();

  trace!(
    "[Search] use {} model to summarize search result docs: {:#?}",
    model_name,
    llm_docs,
  );
  match ai_tool
    .summarize_documents(&query, &model_name, llm_docs, only_context)
    .await
  {
    Ok(resp) => {
      trace!("AI summary search document response: {:?}", resp);
      summaries = resp
        .summaries
        .into_iter()
        .map(|s| Summary {
          content: s.content,
          sources: s.sources,
          highlights: s.highlights,
        })
        .collect();
    },
    Err(err) => error!("AI summary search document failed, error: {:?}", err),
  }

  Ok(SearchSummaryResult { summaries })
}
