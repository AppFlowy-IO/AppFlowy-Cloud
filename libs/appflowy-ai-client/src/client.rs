use crate::dto::{
  AIModel, CalculateSimilarityParams, ChatAnswer, ChatQuestion, CompleteTextResponse,
  CompletionType, CreateChatContext, CustomPrompt, Document, EmbeddingRequest, EmbeddingResponse,
  LocalAIConfig, MessageData, RepeatedLocalAIPackage, RepeatedRelatedQuestion,
  SearchDocumentsRequest, SimilarityResponse, SummarizeRowResponse, TranslateRowData,
  TranslateRowResponse,
};
use crate::error::AIError;

use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest;
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::borrow::Cow;

use std::time::Duration;
use tracing::{info, trace};

const AI_MODEL_HEADER_KEY: &str = "ai-model";

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

  pub async fn completion_text<T: Into<Option<CompletionType>>>(
    &self,
    text: &str,
    completion_type: T,
    custom_prompt: Option<CustomPrompt>,
    model: AIModel,
  ) -> Result<CompleteTextResponse, AIError> {
    let completion_type = completion_type.into();

    if completion_type.is_some() && custom_prompt.is_some() {
      return Err(AIError::InvalidRequest(
        "Cannot specify both completion_type and custom_prompt".to_string(),
      ));
    }

    if text.is_empty() {
      return Err(AIError::InvalidRequest("Empty text".to_string()));
    }

    let params = json!({
      "text": text,
      "type": completion_type.map(|t| t as u8),
      "custom_prompt": custom_prompt,
    });

    let url = format!("{}/completion", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .json(&params)
      .send()
      .await?;
    AIResponse::<CompleteTextResponse>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn stream_completion_text<T: Into<Option<CompletionType>>>(
    &self,
    text: &str,
    completion_type: T,
    custom_prompt: Option<CustomPrompt>,
    model: AIModel,
  ) -> Result<impl Stream<Item = Result<Bytes, AIError>>, AIError> {
    let completion_type = completion_type.into();
    if text.is_empty() {
      return Err(AIError::InvalidRequest("Empty text".to_string()));
    }

    let params = json!({
      "text": text,
      "type": completion_type.map(|t| t as u8),
      "custom_prompt": custom_prompt,
    });

    let url = format!("{}/completion/stream", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
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
      .http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .json(params)
      .send()
      .await?;
    AIResponse::<SummarizeRowResponse>::from_response(resp)
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
      .http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .json(&data)
      .send()
      .await?;
    AIResponse::<TranslateRowResponse>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn embeddings(&self, params: EmbeddingRequest) -> Result<EmbeddingResponse, AIError> {
    let url = format!("{}/embeddings", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .json(&params)
      .send()
      .await?;
    AIResponse::<EmbeddingResponse>::from_response(resp)
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

  pub async fn create_chat_text_context(&self, context: CreateChatContext) -> Result<(), AIError> {
    let url = format!("{}/chat/context/text", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .json(&context)
      .send()
      .await?;
    let _ = AIResponse::<()>::from_response(resp).await?;
    Ok(())
  }

  pub async fn send_question(
    &self,
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
        rag_ids: vec![],
        message_id: Some(question_id.to_string()),
      },
    };
    let url = format!("{}/chat/message", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .json(&json)
      .send()
      .await?;
    AIResponse::<ChatAnswer>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn stream_question(
    &self,
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
        rag_ids,
        message_id: None,
      },
    };
    let url = format!("{}/chat/message/stream", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .timeout(Duration::from_secs(30))
      .json(&json)
      .send()
      .await?;
    AIResponse::<()>::stream_response(resp).await
  }

  pub async fn stream_question_v2(
    &self,
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
        rag_ids,
        message_id: Some(question_id.to_string()),
      },
    };
    let url = format!("{}/v2/chat/message/stream", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .json(&json)
      .timeout(Duration::from_secs(30))
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
      .http_client(Method::GET, &url)?
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .timeout(Duration::from_secs(30))
      .send()
      .await?;
    AIResponse::<RepeatedRelatedQuestion>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_local_ai_package(
    &self,
    platform: &str,
  ) -> Result<RepeatedLocalAIPackage, AIError> {
    let url = format!("{}/local_ai/plugin?platform={platform}", self.url);
    let resp = self.http_client(Method::GET, &url)?.send().await?;
    AIResponse::<RepeatedLocalAIPackage>::from_response(resp)
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

    let resp = self.http_client(Method::GET, &url)?.send().await?;
    AIResponse::<LocalAIConfig>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn calculate_similarity(
    &self,
    params: CalculateSimilarityParams,
  ) -> Result<SimilarityResponse, AIError> {
    let url = format!("{}/similarity", self.url);
    let resp = self
      .http_client(Method::POST, &url)?
      .json(&params)
      .send()
      .await?;
    AIResponse::<SimilarityResponse>::from_response(resp)
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
  T: DeserializeOwned + 'static,
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

  pub async fn stream_response(
    resp: reqwest::Response,
  ) -> Result<impl Stream<Item = Result<Bytes, AIError>>, AIError> {
    let status_code = resp.status();
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
