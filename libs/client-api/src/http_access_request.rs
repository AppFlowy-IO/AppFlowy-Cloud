use client_api_entity::{
  AccessRequestMinimal, ApproveAccessRequestParams, CreateAccessRequestParams,
  access_request_dto::AccessRequest,
};
use reqwest::Method;
use shared_entity::response::AppResponseError;
use uuid::Uuid;

use crate::{Client, process_response_data, process_response_error};

impl Client {
  pub async fn get_access_request(
    &self,
    access_request_id: Uuid,
  ) -> Result<AccessRequest, AppResponseError> {
    let url = format!("{}/api/access-request/{}", self.base_url, access_request_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<AccessRequest>(resp).await
  }

  pub async fn create_access_request(
    &self,
    data: CreateAccessRequestParams,
  ) -> Result<AccessRequestMinimal, AppResponseError> {
    let url = format!("{}/api/access-request", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&data)
      .send()
      .await?;
    process_response_data::<AccessRequestMinimal>(resp).await
  }

  pub async fn approve_access_request(
    &self,
    access_request_id: Uuid,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/access-request/{}/approve",
      self.base_url, access_request_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&ApproveAccessRequestParams { is_approved: true })
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn reject_access_request(
    &self,
    access_request_id: Uuid,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/access-request/{}/approve",
      self.base_url, access_request_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&ApproveAccessRequestParams { is_approved: false })
      .send()
      .await?;
    process_response_error(resp).await
  }
}
