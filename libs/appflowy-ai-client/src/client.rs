use crate::dto::{
  AIModel, CalculateSimilarityParams, ChatAnswer, ChatQuestion, CompleteTextParams,
  CreateChatContext, Document, LocalAIConfig, MessageData, ModelList, QuestionMetadata,
  RepeatedLocalAIPackage, RepeatedRelatedQuestion, ResponseFormat, SearchDocumentsRequest,
  SimilarityResponse, SummarizeRowResponse, TranslateRowData, TranslateRowResponse,
};
use crate::error::AIError;

use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest;
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::time::Duration;
use tracing::{info, trace};

const AI_MODEL_HEADER_KEY: &str = "ai-model";

#[derive(Clone, Debug)]
pub struct AppFlowyAIClient {
  async_client: reqwest::Client,
  url: String,
}

impl AppFlowyAIClient {
  pub fn new(url: &str) -> Self {
    info!("Creating AppFlowyAIClient with url: {}", url);
    let url = url.to_string();
    let async_client = reqwest::Client::new();
    Self { async_client, url }
  }

  pub async fn health_check(&self) -> Result<(), AIError> {
    let url = format!("{}/health", self.url);
    let resp = self.async_http_client(Method::GET, &url)?.send().await?;
    let text = resp.text().await?;
    info!("health response: {:?}", text);
    Ok(())
  }

  pub async fn stream_completion_text(
    &self,
    params: CompleteTextParams,
    model: AIModel,
  ) -> Result<impl Stream<Item = Result<Bytes, AIError>>, AIError> {
    if params.text.is_empty() {
      return Err(AIError::InvalidRequest("Empty text".to_string()));
    }

    let url = format!("{}/completion/stream", self.url);
    let resp = self
      .async_http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .json(&params)
      .send()
      .await?;
    AIResponse::<()>::stream_response(resp).await
  }

  pub async fn summarize_row(
    &self,
    params: &Map<String, Value>,
    model: AIModel,
  ) -> Result<SummarizeRowResponse, AIError> {
    if params.is_empty() {
      return Err(AIError::InvalidRequest("Empty content".to_string()));
    }

    let url = format!("{}/summarize_row", self.url);
    trace!("summarize_row url: {}", url);
    let resp = self
      .async_http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .json(params)
      .send()
      .await?;
    AIResponse::<SummarizeRowResponse>::from_reqwest_response(resp)
      .await?
      .into_data()
  }

  pub async fn translate_row(
    &self,
    data: TranslateRowData,
    model: AIModel,
  ) -> Result<TranslateRowResponse, AIError> {
    let url = format!("{}/translate_row", self.url);
    let resp = self
      .async_http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .json(&data)
      .send()
      .await?;
    AIResponse::<TranslateRowResponse>::from_reqwest_response(resp)
      .await?
      .into_data()
  }

  pub async fn search_documents(
    &self,
    request: &SearchDocumentsRequest,
  ) -> Result<Vec<Document>, AIError> {
    let url = format!("{}/search", self.url);
    let resp = self
      .async_http_client(Method::GET, &url)?
      .query(&request)
      .send()
      .await?;
    AIResponse::<Vec<Document>>::from_reqwest_response(resp)
      .await?
      .into_data()
  }

  pub async fn create_chat_text_context(&self, context: CreateChatContext) -> Result<(), AIError> {
    let url = format!("{}/chat/context/text", self.url);
    let resp = self
      .async_http_client(Method::POST, &url)?
      .json(&context)
      .send()
      .await?;
    let _ = AIResponse::<()>::from_reqwest_response(resp).await?;
    Ok(())
  }

  pub async fn send_question(
    &self,
    workspace_id: &str,
    chat_id: &str,
    question_id: i64,
    content: &str,
    model: &AIModel,
    metadata: Option<Value>,
  ) -> Result<ChatAnswer, AIError> {
    let json = ChatQuestion {
      chat_id: chat_id.to_string(),
      data: MessageData {
        content: content.to_string(),
        metadata,
        message_id: Some(question_id.to_string()),
      },
      format: Default::default(),
      metadata: QuestionMetadata {
        workspace_id: workspace_id.to_string(),
        rag_ids: vec![],
      },
    };
    let url = format!("{}/chat/message", self.url);
    let resp = self
      .async_http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .json(&json)
      .send()
      .await?;
    AIResponse::<ChatAnswer>::from_reqwest_response(resp)
      .await?
      .into_data()
  }

  pub async fn stream_question(
    &self,
    workspace_id: String,
    chat_id: &str,
    content: &str,
    metadata: Option<Value>,
    rag_ids: Vec<String>,
    model: &AIModel,
  ) -> Result<impl Stream<Item = Result<Bytes, AIError>>, AIError> {
    let json = ChatQuestion {
      chat_id: chat_id.to_string(),
      data: MessageData {
        content: content.to_string(),
        metadata,
        message_id: None,
      },
      format: Default::default(),
      metadata: QuestionMetadata {
        workspace_id,
        rag_ids,
      },
    };
    let url = format!("{}/chat/message/stream", self.url);
    let resp = self
      .async_http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .timeout(Duration::from_secs(30))
      .json(&json)
      .send()
      .await?;
    AIResponse::<()>::stream_response(resp).await
  }

  #[allow(clippy::too_many_arguments)]
  pub async fn stream_question_v2(
    &self,
    workspace_id: String,
    chat_id: &str,
    question_id: i64,
    content: &str,
    metadata: Option<Value>,
    rag_ids: Vec<String>,
    model: &AIModel,
  ) -> Result<impl Stream<Item = Result<Bytes, AIError>>, AIError> {
    let json = ChatQuestion {
      chat_id: chat_id.to_string(),
      data: MessageData {
        content: content.to_string(),
        metadata,
        message_id: Some(question_id.to_string()),
      },
      format: ResponseFormat::default(),
      metadata: QuestionMetadata {
        workspace_id,
        rag_ids,
      },
    };
    self.stream_question_v3(model, json, Some(30)).await
  }

  pub async fn stream_question_v3(
    &self,
    model: &AIModel,
    question: ChatQuestion,
    timeout_secs: Option<u64>,
  ) -> Result<impl Stream<Item = Result<Bytes, AIError>>, AIError> {
    let url = format!("{}/v2/chat/message/stream", self.url);
    let resp = self
      .async_http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .json(&question)
      .timeout(Duration::from_secs(timeout_secs.unwrap_or(30)))
      .send()
      .await?;
    AIResponse::<()>::stream_response(resp).await
  }

  pub async fn get_related_question(
    &self,
    chat_id: &str,
    message_id: &i64,
    model: &AIModel,
  ) -> Result<RepeatedRelatedQuestion, AIError> {
    let url = format!("{}/chat/{chat_id}/{message_id}/related_question", self.url);
    let resp = self
      .async_http_client(Method::GET, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .timeout(Duration::from_secs(30))
      .send()
      .await?;
    AIResponse::<RepeatedRelatedQuestion>::from_reqwest_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_local_ai_package(
    &self,
    platform: &str,
  ) -> Result<RepeatedLocalAIPackage, AIError> {
    let url = format!("{}/local_ai/plugin?platform={platform}", self.url);
    let resp = self.async_http_client(Method::GET, &url)?.send().await?;
    AIResponse::<RepeatedLocalAIPackage>::from_reqwest_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_local_ai_config(
    &self,
    platform: &str,
    app_version: Option<String>,
  ) -> Result<LocalAIConfig, AIError> {
    // Start with the base URL including the platform parameter
    let mut url = format!("{}/local_ai/config?platform={}", self.url, platform);

    // If app_version is provided, append it as a query parameter
    if let Some(version) = app_version {
      url = format!("{}&app_version={}", url, version);
    }

    let resp = self.async_http_client(Method::GET, &url)?.send().await?;
    AIResponse::<LocalAIConfig>::from_reqwest_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_model_list(&self) -> Result<ModelList, AIError> {
    let url = format!("{}/model/list", self.url);

    let resp = self.async_http_client(Method::GET, &url)?.send().await?;
    AIResponse::<ModelList>::from_reqwest_response(resp)
      .await?
      .into_data()
  }

  pub async fn calculate_similarity(
    &self,
    params: CalculateSimilarityParams,
  ) -> Result<SimilarityResponse, AIError> {
    let url = format!("{}/similarity", self.url);
    let resp = self
      .async_http_client(Method::POST, &url)?
      .json(&params)
      .send()
      .await?;
    AIResponse::<SimilarityResponse>::from_reqwest_response(resp)
      .await?
      .into_data()
  }

  fn async_http_client(&self, method: Method, url: &str) -> Result<RequestBuilder, AIError> {
    let request_builder = self.async_client.request(method, url);
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
  T: DeserializeOwned + 'static,
{
  pub async fn from_reqwest_response(resp: reqwest::Response) -> Result<Self, anyhow::Error> {
    let status_code = resp.status();
    if !status_code.is_success() {
      let body = resp.text().await?;
      anyhow::bail!("error code: {}, {}", status_code, body)
    }

    let bytes = resp.bytes().await?;
    let resp = serde_json::from_slice(&bytes)?;
    Ok(resp)
  }

  pub fn from_ur_response(resp: ureq::Response) -> Result<Self, anyhow::Error> {
    let status_code = resp.status();
    if status_code != 200 {
      let body = resp.into_string()?;
      anyhow::bail!("error code: {}, {}", status_code, body)
    }

    let resp = resp.into_json()?;
    Ok(resp)
  }

  pub fn into_data(self) -> Result<T, AIError> {
    match self.data {
      None => Err(AIError::InvalidRequest("Empty payload".to_string())),
      Some(data) => Ok(data),
    }
  }

  pub async fn stream_response(
    resp: reqwest::Response,
  ) -> Result<impl Stream<Item = Result<Bytes, AIError>>, AIError> {
    let status_code = resp.status();
    if status_code.is_server_error() {
      let body = resp.text().await?;
      return Err(AIError::ServiceUnavailable(body));
    }

    if !status_code.is_success() {
      let body = resp.text().await?;
      return Err(AIError::InvalidRequest(body));
    }
    let stream = resp
      .bytes_stream()
      .map(|item| item.map_err(|err| AIError::Internal(err.into())));
    Ok(stream)
  }
}
impl From<reqwest::Error> for AIError {
  fn from(error: reqwest::Error) -> Self {
    if error.is_connect() {
      return AIError::ServiceUnavailable(error.to_string());
    }

    if error.is_timeout() {
      return AIError::RequestTimeout(error.to_string());
    }

    // Handle request-related errors
    if let Some(status_code) = error.status() {
      if error.is_request() {
        match status_code {
          StatusCode::PAYLOAD_TOO_LARGE => {
            return AIError::PayloadTooLarge(error.to_string());
          },
          status_code if status_code.is_server_error() => {
            return AIError::ServiceUnavailable(error.to_string());
          },
          _ => {
            return AIError::InvalidRequest(format!("{:?}", error));
          },
        }
      }
    }
    AIError::Internal(error.into())
  }
}

pub async fn collect_stream_text(stream: impl Stream<Item = Result<Bytes, AIError>>) -> String {
  let stream = stream.map(|item| {
    item.map(|bytes| {
      String::from_utf8(bytes.to_vec())
        .map(|s| s.replace('\n', ""))
        .unwrap()
    })
  });

  let lines: Vec<String> = stream.map(|message| message.unwrap()).collect().await;
  lines.join("")
}
