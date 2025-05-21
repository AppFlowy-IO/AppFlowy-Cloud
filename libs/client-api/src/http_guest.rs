use client_api_entity::guest_dto::{
  ListSharedViewResponse, RevokeSharedViewAccessRequest, ShareViewWithGuestRequest,
  SharedViewDetails,
};
use reqwest::Method;
use serde_json::json;
use shared_entity::response::AppResponseError;
use uuid::Uuid;

use crate::{Client, process_response_data, process_response_error};

impl Client {
  pub async fn share_view_with_guest(
    &self,
    workspace_id: &Uuid,
    params: &ShareViewWithGuestRequest,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/sharing/workspace/{}/view",
      self.base_url, workspace_id,
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn revoke_shared_view_access(
    &self,
    workspace_id: &Uuid,
    view_id: &Uuid,
    params: &RevokeSharedViewAccessRequest,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/sharing/workspace/{}/view/{}/revoke-access",
      self.base_url, workspace_id, view_id,
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn get_shared_views(
    &self,
    workspace_id: &Uuid,
  ) -> Result<ListSharedViewResponse, AppResponseError> {
    let url = format!(
      "{}/api/sharing/workspace/{}/view",
      self.base_url, workspace_id,
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&json!({}))
      .send()
      .await?;
    process_response_data(resp).await
  }

  pub async fn get_shared_view_details(
    &self,
    workspace_id: &Uuid,
    view_id: &Uuid,
  ) -> Result<SharedViewDetails, AppResponseError> {
    let url = format!(
      "{}/api/sharing/workspace/{}/view/{}",
      self.base_url, workspace_id, view_id,
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&json!({}))
      .send()
      .await?;
    process_response_data(resp).await
  }
}
