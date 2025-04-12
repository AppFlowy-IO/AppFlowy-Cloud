use app_error::ErrorCode;
use reqwest::Method;
use shared_entity::dto::search_dto::{
  SearchDocumentResponseItem, SearchResult, SearchSummaryResult, SummarySearchResultRequest,
};
use shared_entity::response::AppResponseError;
use uuid::Uuid;

use crate::{process_response_data, Client};

impl Client {
  /// If `score` is `None`, it will use the score from the server. High score means more relevant.
  /// score range is 0.0 to 1.0
  pub async fn search_documents<T: Into<Option<f32>>>(
    &self,
    workspace_id: &Uuid,
    query: &str,
    limit: u32,
    preview_size: u32,
    score: T,
  ) -> Result<Vec<SearchDocumentResponseItem>, AppResponseError> {
    let mut raw_query = Vec::with_capacity(4);
    raw_query.push(("query", query.to_string()));
    raw_query.push(("limit", limit.to_string()));
    raw_query.push(("preview_size", preview_size.to_string()));

    if let Some(score_limit) = score.into() {
      raw_query.push(("score", score_limit.to_string()));
    }

    let query = serde_urlencoded::to_string(raw_query)
      .map_err(|err| AppResponseError::new(ErrorCode::InvalidRequest, err.to_string()))?;

    let url = format!("{}/api/search/{workspace_id}?{query}", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<Vec<SearchDocumentResponseItem>>(resp).await
  }

  /// High score means more relevant
  pub async fn generate_search_summary(
    &self,
    workspace_id: &Uuid,
    query: &str,
    search_results: Vec<SearchResult>,
  ) -> Result<SearchSummaryResult, AppResponseError> {
    let payload = SummarySearchResultRequest {
      query: query.to_string(),
      search_results,
      only_context: true,
    };

    let url = format!("{}/api/search/{workspace_id}/summary", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&payload)
      .send()
      .await?;
    process_response_data::<SearchSummaryResult>(resp).await
  }
}
