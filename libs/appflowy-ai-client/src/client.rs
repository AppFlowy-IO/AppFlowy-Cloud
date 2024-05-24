use crate::dto::{
  ChatAnswer, ChatQuestion, CompleteTextResponse, CompletionType, Document, MessageData,
  SearchDocumentsRequest, SummarizeRowResponse, TranslateRowResponse,
};
use crate::error::AIError;
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use tracing::{info, trace};

#[derive(Clone, Debug)]
pub struct AppFlowyAIClient {
  client: reqwest::Client,
  url: String,
}

impl AppFlowyAIClient {
  pub fn new(url: &str) -> Self {
    info!("Creating AppFlowyAIClient with url: {}", url);
    let url = url.to_string();
    let client = reqwest::Client::new();
    Self { client, url }
  }

  pub async fn health_check(&self) -> Result<(), AIError> {
    let url = format!("{}/health", self.url);
    let resp = self.http_client(Method::GET, &url)?.send().await?;
    let text = resp.text().await?;
    info!("health response: {:?}", text);
    Ok(())
  }

  pub async fn completion_text(
    &self,
    text: &str,
    completion_type: CompletionType,
  ) -> Result<CompleteTextResponse, AIError> {
    if text.is_empty() {
      return Err(AIError::InvalidRequest("Empty text".to_string()));
    }

    let params = json!({
      "text": text,
      "type": completion_type as u8,
    });

    let url = format!("{}/completion", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .json(&params)
      .send()
      .await?;
    AIResponse::<CompleteTextResponse>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn summarize_row(
    &self,
    params: &Map<String, Value>,
  ) -> Result<SummarizeRowResponse, AIError> {
    if params.is_empty() {
      return Err(AIError::InvalidRequest("Empty content".to_string()));
    }

    let url = format!("{}/summarize_row", self.url);
    trace!("summarize_row url: {}", url);
    let resp = self
      .http_client(Method::POST, &url)?
      .json(params)
      .send()
      .await?;
    AIResponse::<SummarizeRowResponse>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn translate_row(&self, json: Value) -> Result<TranslateRowResponse, AIError> {
    let url = format!("{}/translate_row", self.url);
    trace!("translate_row url: {}", url);
    let resp = self
      .http_client(Method::POST, &url)?
      .json(&json)
      .send()
      .await?;
    AIResponse::<TranslateRowResponse>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn index_documents(&self, documents: &[Document]) -> Result<(), AIError> {
    let url = format!("{}/index_documents", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .json(&documents)
      .send()
      .await?;
    let status_code = resp.status();
    if !status_code.is_success() {
      let body = resp.text().await?;
      return Err(anyhow::anyhow!("error: {}, {}", status_code, body).into());
    }
    Ok(())
  }

  pub async fn search_documents(
    &self,
    request: &SearchDocumentsRequest,
  ) -> Result<Vec<Document>, AIError> {
    let url = format!("{}/search", self.url);
    let resp = self
      .http_client(Method::GET, &url)?
      .query(&request)
      .send()
      .await?;
    AIResponse::<Vec<Document>>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn send_question(&self, chat_id: &str, content: &str) -> Result<ChatAnswer, AIError> {
    let json = ChatQuestion {
      chat_id: chat_id.to_string(),
      data: MessageData {
        content: content.to_string(),
      },
    };
    let url = format!("{}/chat/message", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .json(&json)
      .send()
      .await?;
    AIResponse::<ChatAnswer>::from_response(resp)
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
      anyhow::bail!("error code: {}, {}", status_code, body)
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

impl From<reqwest::Error> for AIError {
  fn from(error: reqwest::Error) -> Self {
    if error.is_timeout() {
      return AIError::RequestTimeout(error.to_string());
    }

    if error.is_request() {
      return if error.status() == Some(StatusCode::PAYLOAD_TOO_LARGE) {
        AIError::PayloadTooLarge(error.to_string())
      } else {
        AIError::InvalidRequest(format!("{:?}", error))
      };
    }
    AIError::Internal(error.into())
  }
}
