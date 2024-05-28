use app_error::ErrorCode;
use reqwest::Method;
use shared_entity::dto::search_dto::SearchDocumentResponseItem;
use shared_entity::response::{AppResponse, AppResponseError};

use crate::http::log_request_id;
use crate::Client;

impl Client {
  pub async fn search_documents(
    &self,
    workspace_id: &str,
    query: &str,
    limit: u32,
    preview_size: u32,
  ) -> Result<Vec<SearchDocumentResponseItem>, AppResponseError> {
    let query = serde_urlencoded::to_string([
      ("query", query),
      ("limit", &limit.to_string()),
      ("preview_size", &preview_size.to_string()),
    ])
    .map_err(|err| AppResponseError::new(ErrorCode::InvalidRequest, err.to_string()))?;
    let url = format!("{}/api/search/{workspace_id}/?{query}", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<Vec<SearchDocumentResponseItem>>::from_response(resp)
      .await?
      .into_data()
  }
}
