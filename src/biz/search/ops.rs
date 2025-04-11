use crate::biz::collab::folder_view::PrivateSpaceAndTrashViews;
use crate::biz::collab::utils::get_latest_collab_folder;
use crate::{
  api::metrics::RequestMetrics, biz::collab::folder_view::private_space_and_trash_view_ids,
};
use app_error::AppError;
use appflowy_ai_client::dto::EmbeddingModel;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use collab_folder::{Folder, View};
use database::collab::GetCollabOrigin;
use database::index::{search_documents, SearchDocumentParams};
use indexer::scheduler::IndexerScheduler;
use indexer::vector::embedder::{CreateEmbeddingRequestArgs, EmbeddingInput, EncodingFormat};
use llm_client::chat::{AITool, LLMDocument};
use serde_json::json;
use shared_entity::dto::search_dto::{
  SearchContentType, SearchDocumentRequest, SearchDocumentResponseItem, SearchResult, Summary,
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

#[allow(clippy::too_many_arguments)]
pub async fn search_document(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  indexer_scheduler: &Arc<IndexerScheduler>,
  uid: i64,
  workspace_uuid: Uuid,
  request: SearchDocumentRequest,
  metrics: &RequestMetrics,
  ai_tool: Option<AITool>,
) -> Result<SearchResult, AppError> {
  let embeddings_request = CreateEmbeddingRequestArgs::default()
    .model(EmbeddingModel::default_model().to_string())
    .input(EmbeddingInput::String(request.query.clone()))
    .encoding_format(EncodingFormat::Float)
    .dimensions(EmbeddingModel::default_model().default_dimensions())
    .build()
    .map_err(|err| AppError::Unhandled(err.to_string()))?;

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

  let embedding = embeddings_resp
    .data
    .pop()
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("OpenAI returned no embeddings")))?;

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
  let preview = request.preview_size.unwrap_or(500) as i32;
  let results = search_documents(
    pg_pool,
    SearchDocumentParams {
      user_id: uid,
      workspace_id: workspace_uuid,
      limit: request.limit.unwrap_or(10) as i32,
      preview,
      embedding: embedding.embedding,
      searchable_view_ids: searchable_view_ids.into_iter().collect(),
      score: request.score,
    },
    total_tokens,
  )
  .await?;

  trace!(
    "user {} search request in workspace {} returned {} results for query: `{}`",
    uid,
    workspace_uuid,
    results.len(),
    request.query
  );

  let mut summary = vec![];
  if !results.is_empty() {
    if let Some(ai_chat) = ai_tool {
      let model_name = "gpt-4o-mini";
      trace!("using {} model to summarize search results", model_name);
      match ai_chat
        .summary_documents(
          &request.query,
          model_name,
          &results
            .iter()
            .map(|result| {
              let metadata = json!({
                  "id": result.object_id,
                  "source": "appflowy",
                  "name": "document",
              });
              LLMDocument::new(result.content.clone(), metadata)
            })
            .collect::<Vec<_>>(),
          request.only_context,
        )
        .await
      {
        Ok(resp) => {
          trace!("AI summary search document response: {:?}", resp);
          summary = resp
            .summaries
            .into_iter()
            .map(|summary| Summary {
              content: summary.content,
              metadata: summary.metadata,
              score: summary.score,
            })
            .collect::<Vec<_>>();
        },
        Err(err) => {
          error!("AI summary search document failed, error: {:?}", err);
        },
      }
    }
  }

  Ok(SearchResult {
    summaries: summary,
    items: results
      .into_iter()
      .map(|item| SearchDocumentResponseItem {
        object_id: item.object_id,
        workspace_id: item.workspace_id,
        score: item.score,
        content_type: SearchContentType::from_record(item.content_type),
        preview: Some(item.content.chars().take(preview as usize).collect()),
        created_by: item.created_by,
        created_at: item.created_at,
      })
      .collect(),
  })
}
