use client_api_entity::guest_dto::{
  ListSharedViewResponse, RevokeSharedViewAccessRequest, ShareViewWithGuestRequest,
  SharedViewDetails, SharedViewDetailsRequest,
};
use reqwest::Method;
use shared_entity::response::AppResponseError;
use uuid::Uuid;

use crate::{process_response_data, process_response_error, Client};

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
      .send()
      .await?;
    process_response_data(resp).await
  }

  pub async fn get_shared_view_details(
    &self,
    workspace_id: &Uuid,
    view_id: &Uuid,
    ancestor_view_ids: &[Uuid],
  ) -> Result<SharedViewDetails, AppResponseError> {
    let url = format!(
      "{}/api/sharing/workspace/{}/view/{}/access-details",
      self.base_url, workspace_id, view_id,
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&SharedViewDetailsRequest {
        ancestor_view_ids: ancestor_view_ids.to_vec(),
      })
      .send()
      .await?;
    process_response_data(resp).await
  }
}
