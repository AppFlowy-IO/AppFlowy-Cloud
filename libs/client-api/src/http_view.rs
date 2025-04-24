use client_api_entity::workspace_dto::{
  AddRecentPagesParams, AppendBlockToPageParams, CreateFolderViewParams,
  CreatePageDatabaseViewParams, CreatePageParams, CreateSpaceParams, DuplicatePageParams,
  FavoritePageParams, MovePageParams, Page, PageCollab, PublishPageParams, Space, UpdatePageParams,
  UpdateSpaceParams,
};
use reqwest::Method;
use serde_json::json;
use shared_entity::response::AppResponseError;
use uuid::Uuid;

use crate::{process_response_data, process_response_error, Client};

impl Client {
  pub async fn create_folder_view(
    &self,
    workspace_id: Uuid,
    params: &CreateFolderViewParams,
  ) -> Result<Page, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/folder-view",
      self.base_url, workspace_id,
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;
    process_response_data::<Page>(resp).await
  }

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
    process_response_data::<Page>(resp).await
  }

  pub async fn favorite_page_view(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
    params: &FavoritePageParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/page-view/{}/favorite",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn move_workspace_page_view(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
    params: &MovePageParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/page-view/{}/move",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn move_workspace_page_view_to_trash(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
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
    process_response_error(resp).await
  }

  pub async fn restore_workspace_page_view_from_trash(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
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
    process_response_error(resp).await
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
    process_response_error(resp).await
  }

  pub async fn add_recent_pages(
    &self,
    workspace_id: Uuid,
    params: &AddRecentPagesParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/add-recent-pages",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn delete_workspace_page_view_from_trash(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/trash/{}",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn delete_all_workspace_page_views_from_trash(
    &self,
    workspace_id: Uuid,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/delete-all-pages-from-trash",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&json!({}))
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn update_workspace_page_view(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
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
    process_response_error(resp).await
  }

  pub async fn get_workspace_page_view(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
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
    process_response_data::<PageCollab>(resp).await
  }

  pub async fn publish_page(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
    params: &PublishPageParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/page-view/{}/publish",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn unpublish_page(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/page-view/{}/unpublish",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&json!({}))
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn create_space(
    &self,
    workspace_id: Uuid,
    params: &CreateSpaceParams,
  ) -> Result<Space, AppResponseError> {
    let url = format!("{}/api/workspace/{}/space", self.base_url, workspace_id,);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;
    process_response_data::<Space>(resp).await
  }

  pub async fn update_space(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
    params: &UpdateSpaceParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/space/{}",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::PATCH, &url)
      .await?
      .json(params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn append_block_to_page(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
    params: &AppendBlockToPageParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/page-view/{}/append-block",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn create_database_view(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
    params: &CreatePageDatabaseViewParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/page-view/{}/database-view",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn duplicate_view_and_children(
    &self,
    workspace_id: Uuid,
    view_id: &Uuid,
    params: &DuplicatePageParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/page-view/{}/duplicate",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;
    process_response_error(resp).await
  }
}
