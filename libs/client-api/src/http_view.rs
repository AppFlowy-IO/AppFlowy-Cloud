use client_api_entity::workspace_dto::PageCollab;
use reqwest::Method;
use shared_entity::response::{AppResponse, AppResponseError};
use uuid::Uuid;

use crate::Client;

impl Client {
  pub async fn get_workspace_page_view(
    &self,
    workspace_id: Uuid,
    view_id: Uuid,
  ) -> Result<PageCollab, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/page-view/{}",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<PageCollab>::from_response(resp)
      .await?
      .into_data()
  }
}
