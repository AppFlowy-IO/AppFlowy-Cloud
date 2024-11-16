use client_api_entity::workspace_dto::{CreatePageParams, Page, PageCollab, UpdatePageParams};
use reqwest::Method;
use serde_json::json;
use shared_entity::response::{AppResponse, AppResponseError};
use uuid::Uuid;

use crate::Client;

impl Client {
  pub async fn create_workspace_page_view(
    &self,
    workspace_id: Uuid,
    params: &CreatePageParams,
  ) -> Result<Page, AppResponseError> {
    let url = format!("{}/api/workspace/{}/page-view", self.base_url, workspace_id,);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;
    AppResponse::<Page>::from_response(resp).await?.into_data()
  }

  pub async fn move_workspace_page_view_to_trash(
    &self,
    workspace_id: Uuid,
    view_id: String,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/page-view/{}/move-to-trash",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&json!({}))
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn restore_workspace_page_view_from_trash(
    &self,
    workspace_id: Uuid,
    view_id: &str,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/page-view/{}/restore-from-trash",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&json!({}))
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn restore_all_workspace_page_views_from_trash(
    &self,
    workspace_id: Uuid,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/restore-all-pages-from-trash",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&json!({}))
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn update_workspace_page_view(
    &self,
    workspace_id: Uuid,
    view_id: &str,
    params: &UpdatePageParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/page-view/{}",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::PATCH, &url)
      .await?
      .json(params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn get_workspace_page_view(
    &self,
    workspace_id: Uuid,
    view_id: &str,
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
