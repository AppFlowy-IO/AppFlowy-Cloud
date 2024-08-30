use crate::http::log_request_id;
use crate::Client;
use reqwest::Method;
use shared_entity::dto::ai_dto::{
  CompleteTextParams, CompleteTextResponse, LocalAIConfig, SummarizeRowParams,
  SummarizeRowResponse, TranslateRowParams, TranslateRowResponse,
};
use shared_entity::response::{AppResponse, AppResponseError};
use std::time::Duration;
use tracing::instrument;

impl Client {
  #[instrument(level = "info", skip_all)]
  pub async fn summarize_row(
    &self,
    params: SummarizeRowParams,
  ) -> Result<SummarizeRowResponse, AppResponseError> {
    let url = format!(
      "{}/api/ai/{}/summarize_row",
      self.base_url, params.workspace_id
    );

    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;

    log_request_id(&resp);
    AppResponse::<SummarizeRowResponse>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "info", skip_all)]
  pub async fn translate_row(
    &self,
    params: TranslateRowParams,
  ) -> Result<TranslateRowResponse, AppResponseError> {
    let url = format!(
      "{}/api/ai/{}/translate_row",
      self.base_url, params.workspace_id
    );

    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .timeout(Duration::from_secs(30))
      .send()
      .await?;

    log_request_id(&resp);
    AppResponse::<TranslateRowResponse>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "info", skip_all)]
  pub async fn completion_text(
    &self,
    workspace_id: &str,
    params: CompleteTextParams,
  ) -> Result<CompleteTextResponse, AppResponseError> {
    let url = format!("{}/api/ai/{}/complete", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .timeout(Duration::from_secs(30))
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<CompleteTextResponse>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn get_local_ai_config(
    &self,
    workspace_id: &str,
    platform: &str,
  ) -> Result<LocalAIConfig, AppResponseError> {
    let client_version = self.client_version.to_string();
    let url = format!(
      "{}/api/ai/{}/local/config?platform={platform}&app_version={client_version}",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<LocalAIConfig>::from_response(resp)
      .await?
      .into_data()
  }
}
