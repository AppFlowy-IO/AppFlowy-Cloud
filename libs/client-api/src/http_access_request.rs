use client_api_entity::{
  access_request_dto::AccessRequest, AccessRequestMinimal, ApproveAccessRequestParams,
  CreateAccessRequestParams,
};
use reqwest::Method;
use shared_entity::response::{AppResponse, AppResponseError};
use uuid::Uuid;

use crate::Client;

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
    AppResponse::<AccessRequest>::from_response(resp)
      .await?
      .into_data()
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
    AppResponse::<AccessRequestMinimal>::from_response(resp)
      .await?
      .into_data()
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
    AppResponse::<()>::from_response(resp).await?.into_error()
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
    AppResponse::<()>::from_response(resp).await?.into_error()
  }
}
