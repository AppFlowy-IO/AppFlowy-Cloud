use crate::http::log_request_id;
use crate::Client;
use reqwest::Method;
use shared_entity::dto::ai_dto::{
  CompleteTextParams, CompleteTextResponse, SummarizeRowParams, SummarizeRowResponse,
};
use shared_entity::response::{AppResponse, AppResponseError};
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
  pub async fn completion_text(
    &self,
    workspace_id: &str,
    params: CompleteTextParams,
  ) -> Result<CompleteTextResponse, AppResponseError> {
    let url = format!("{}/api/ai/{}/complete_text", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<CompleteTextResponse>::from_response(resp)
      .await?
      .into_data()
  }
}
