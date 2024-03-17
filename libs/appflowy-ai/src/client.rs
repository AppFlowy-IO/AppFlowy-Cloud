use crate::entity::{SummarizeRow, TranslateRow};
use crate::error::AIError;
use reqwest::{Method, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;

#[derive(Clone, Debug)]
pub struct AppFlowyAIClient {
  client: reqwest::Client,
  url: String,
}

impl AppFlowyAIClient {
  pub fn new(url: &str) -> Self {
    let url = url.to_string();
    let client = reqwest::Client::new();
    Self { client, url }
  }

  pub async fn summarize_row(&self, json: Value) -> Result<SummarizeRow, AIError> {
    let url = format!("{}/summarize_row", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .json(&json)
      .send()
      .await?;
    AIResponse::<SummarizeRow>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn translate_row(&self, json: Value) -> Result<TranslateRow, AIError> {
    let url = format!("{}/translate_row", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .json(&json)
      .send()
      .await?;
    AIResponse::<TranslateRow>::from_response(resp)
      .await?
      .into_data()
  }

  fn http_client(&self, method: Method, url: &str) -> Result<RequestBuilder, AIError> {
    let request_builder = self.client.request(method, url);
    Ok(request_builder)
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResponse<T> {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub data: Option<T>,

  #[serde(default)]
  pub message: Cow<'static, str>,
}

impl<T> AIResponse<T>
where
  T: serde::de::DeserializeOwned + 'static,
{
  pub async fn from_response(resp: reqwest::Response) -> Result<Self, anyhow::Error> {
    let status_code = resp.status();
    if !status_code.is_success() {
      let body = resp.text().await?;
      anyhow::bail!("got error code: {}, body: {}", status_code, body)
    }

    let bytes = resp.bytes().await?;
    let resp = serde_json::from_slice(&bytes)?;
    Ok(resp)
  }

  pub fn into_data(self) -> Result<T, AIError> {
    match self.data {
      None => Err(AIError::InvalidRequest("Empty payload".to_string())),
      Some(data) => Ok(data),
    }
  }
}
