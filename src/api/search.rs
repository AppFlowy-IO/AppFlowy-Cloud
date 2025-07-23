use crate::biz::authentication::jwt::Authorization;
use crate::biz::search::{search_document, summarize_search_results};
use crate::state::AppState;
use access_control::act::Action;
use actix_web::web::{Data, Json, Query};
use actix_web::{web, Scope};
use async_openai::config::{AzureConfig, OpenAIConfig};

use llm_client::chat::{AITool, AzureOpenAIChat, OpenAIChat};
use shared_entity::dto::search_dto::{
  SearchDocumentRequest, SearchDocumentResponseItem, SearchSummaryResult,
  SummarySearchResultRequest,
};
use shared_entity::response::{AppResponse, JsonAppResponse};
use uuid::Uuid;

pub fn search_scope() -> Scope {
  web::scope("/api/search")
    .service(web::resource("{workspace_id}").route(web::get().to(document_search_handler)))
    .service(
      web::resource("/{workspace_id}/summary").route(web::get().to(summary_search_results_handler)),
    )
}

#[tracing::instrument(skip(state, auth, payload), err)]
async fn document_search_handler(
  auth: Authorization,
  path: web::Path<Uuid>,
  payload: Query<SearchDocumentRequest>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<Vec<SearchDocumentResponseItem>>> {
  let workspace_id = path.into_inner();
  let request = payload.into_inner();
  let user_uuid = auth.uuid()?;
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  state
    .workspace_access_control
    .enforce_action(&uid, &workspace_id, Action::Read)
    .await?;
  let metrics = &*state.metrics.request_metrics;
  let resp = search_document(
    &state.pg_pool,
    &state.ws_server,
    &state.indexer_scheduler,
    uid,
    workspace_id,
    request,
    metrics,
  )
  .await?;
  Ok(AppResponse::Ok().with_data(resp).into())
}

#[tracing::instrument(skip(state, auth, payload), err)]
async fn summary_search_results_handler(
  auth: Authorization,
  path: web::Path<Uuid>,
  payload: Json<SummarySearchResultRequest>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<SearchSummaryResult>> {
  let workspace_id = path.into_inner();
  let request = payload.into_inner();
  let user_uuid = auth.uuid()?;
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  state
    .workspace_access_control
    .enforce_action(&uid, &workspace_id, Action::Read)
    .await?;

  let ai_tool = create_ai_tool(&state.config.azure_ai_config, &state.config.open_ai_config);
  let result = summarize_search_results(ai_tool, request).await?;
  Ok(AppResponse::Ok().with_data(result).into())
}

pub fn create_ai_tool(
  azure_ai_config: &Option<AzureConfig>,
  open_ai_config: &Option<OpenAIConfig>,
) -> Option<AITool> {
  if let Some(config) = &azure_ai_config {
    return Some(AITool::AzureOpenAI(AzureOpenAIChat::new(config.clone())));
  }

  if let Some(config) = &open_ai_config {
    return Some(AITool::OpenAI(OpenAIChat::new(config.clone())));
  }
  None
}
