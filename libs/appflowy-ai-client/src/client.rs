use crate::dto::{
  AIModel, ChatAnswer, ChatQuestion, CompleteTextResponse, CompletionType, CreateTextChatContext,
  Document, EmbeddingRequest, EmbeddingResponse, LocalAIConfig, MessageData,
  RepeatedLocalAIPackage, RepeatedRelatedQuestion, SearchDocumentsRequest, SummarizeRowResponse,
  TranslateRowData, TranslateRowResponse,
};
use crate::error::AIError;

use futures::{ready, Stream, StreamExt, TryStreamExt};
use reqwest;
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, StreamDeserializer, Value};

use anyhow::anyhow;
use pin_project::pin_project;
use serde::de::DeserializeOwned;
use serde_json::de::SliceRead;
use std::borrow::Cow;
use std::marker::PhantomData;
use std::pin::Pin;

use bytes::Bytes;
use std::task::{Context, Poll};
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

  pub async fn completion_text(
    &self,
    text: &str,
    completion_type: CompletionType,
    model: AIModel,
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
      .header(AI_MODEL_HEADER_KEY, model.to_str())
      .json(&params)
      .send()
      .await?;
    AIResponse::<CompleteTextResponse>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn stream_completion_text(
    &self,
    text: &str,
    completion_type: CompletionType,
    model: AIModel,
  ) -> Result<impl Stream<Item = Result<Bytes, AIError>>, AIError> {
    if text.is_empty() {
      return Err(AIError::InvalidRequest("Empty text".to_string()));
    }

    let params = json!({
      "text": text,
      "type": completion_type as u8,
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

  pub async fn create_chat_text_context(
    &self,
    context: CreateTextChatContext,
  ) -> Result<(), AIError> {
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
    content: &str,
    model: &AIModel,
  ) -> Result<ChatAnswer, AIError> {
    let json = ChatQuestion {
      chat_id: chat_id.to_string(),
      data: MessageData {
        content: content.to_string(),
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
    model: &AIModel,
  ) -> Result<impl Stream<Item = Result<Bytes, AIError>>, AIError> {
    let json = ChatQuestion {
      chat_id: chat_id.to_string(),
      data: MessageData {
        content: content.to_string(),
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
    content: &str,
    model: &AIModel,
  ) -> Result<impl Stream<Item = Result<Bytes, AIError>>, AIError> {
    let json = ChatQuestion {
      chat_id: chat_id.to_string(),
      data: MessageData {
        content: content.to_string(),
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

  pub async fn json_stream_response(
    resp: reqwest::Response,
  ) -> Result<impl Stream<Item = Result<T, AIError>>, AIError> {
    let status_code = resp.status();
    if !status_code.is_success() {
      let body = resp.text().await?;
      return Err(AIError::Internal(anyhow!(body)));
    }

    let stream = resp.bytes_stream().map_err(AIError::from);
    Ok(JsonStream::new(stream))
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

#[pin_project]
pub struct JsonStream<T> {
  stream: Pin<Box<dyn Stream<Item = Result<Bytes, AIError>> + Send>>,
  buffer: Vec<u8>,
  _marker: PhantomData<T>,
}

impl<T> JsonStream<T> {
  pub fn new<S>(stream: S) -> Self
  where
    S: Stream<Item = Result<Bytes, AIError>> + Send + 'static,
  {
    JsonStream {
      stream: Box::pin(stream),
      buffer: Vec::new(),
      _marker: PhantomData,
    }
  }
}

impl<T> Stream for JsonStream<T>
where
  T: DeserializeOwned,
{
  type Item = Result<T, AIError>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let this = self.project();

    match ready!(this.stream.as_mut().poll_next(cx)) {
      Some(Ok(bytes)) => {
        this.buffer.extend_from_slice(&bytes);
        let de = StreamDeserializer::new(SliceRead::new(this.buffer));
        let mut iter = de.into_iter();
        if let Some(result) = iter.next() {
          match result {
            Ok(value) => {
              let remaining = iter.byte_offset();
              this.buffer.drain(0..remaining);
              Poll::Ready(Some(Ok(value)))
            },
            Err(err) => {
              if err.is_eof() {
                Poll::Pending
              } else {
                Poll::Ready(Some(Err(AIError::Internal(err.into()))))
              }
            },
          }
        } else {
          Poll::Pending
        }
      },
      Some(Err(err)) => Poll::Ready(Some(Err(err))),
      None => Poll::Ready(None),
    }
  }
}
