use app_error::ErrorCode;
use reqwest::Method;
use shared_entity::dto::search_dto::SearchDocumentResponseItem;
use shared_entity::response::AppResponseError;
use uuid::Uuid;

use crate::{process_response_data, Client};

impl Client {
  pub async fn search_documents(
    &self,
    workspace_id: &Uuid,
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
    let url = format!("{}/api/v2/search/{workspace_id}?{query}", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<Vec<SearchDocumentResponseItem>>(resp).await
  }
}
