use crate::http_chat::CompletionStream;
use crate::{process_response_data, Client};
use bytes::Bytes;
use futures_core::Stream;
use reqwest::Method;
use shared_entity::dto::ai_dto::{
  CompleteTextParams, LocalAIConfig, ModelList, SummarizeRowParams, SummarizeRowResponse,
  TranslateRowParams, TranslateRowResponse,
};
use shared_entity::response::{AppResponse, AppResponseError};
use std::time::Duration;
use tracing::instrument;
use uuid::Uuid;

impl Client {
  pub async fn stream_completion_text(
    &self,
    workspace_id: &str,
    params: CompleteTextParams,
  ) -> Result<impl Stream<Item = Result<Bytes, AppResponseError>>, AppResponseError> {
    let url = format!("{}/api/ai/{}/complete/stream", self.base_url, workspace_id);
    let resp = self
      .http_client_with_model(Method::POST, &url, None)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::answer_response_stream(resp).await
  }

  pub async fn stream_completion_v2(
    &self,
    workspace_id: &Uuid,
    params: CompleteTextParams,
    ai_model: Option<String>,
  ) -> Result<CompletionStream, AppResponseError> {
    let url = format!(
      "{}/api/ai/{}/v2/complete/stream",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_model(Method::POST, &url, ai_model)
      .await?
      .json(&params)
      .send()
      .await?;
    let stream = AppResponse::<serde_json::Value>::json_response_stream(resp).await?;
    Ok(CompletionStream::new(stream))
  }

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
      .http_client_with_model(Method::POST, &url, None)
      .await?
      .json(&params)
      .send()
      .await?;

    process_response_data::<SummarizeRowResponse>(resp).await
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
      .http_client_with_model(Method::POST, &url, None)
      .await?
      .json(&params)
      .timeout(Duration::from_secs(30))
      .send()
      .await?;

    process_response_data::<TranslateRowResponse>(resp).await
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
    process_response_data::<LocalAIConfig>(resp).await
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_model_list(&self, workspace_id: &Uuid) -> Result<ModelList, AppResponseError> {
    let url = format!("{}/api/ai/{workspace_id}/model/list", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<ModelList>(resp).await
  }
}
