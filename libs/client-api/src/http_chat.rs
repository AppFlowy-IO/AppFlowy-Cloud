use crate::{process_response_data, process_response_error, Client};

use app_error::AppError;
use client_api_entity::chat_dto::{
  ChatMessage, CreateAnswerMessageParams, CreateChatMessageParams, CreateChatParams, MessageCursor,
  RepeatedChatMessage, RepeatedChatMessageWithAuthorUuid, UpdateChatMessageContentParams,
};
use futures_core::{ready, Stream};
use pin_project::pin_project;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shared_entity::dto::ai_dto::{
  CalculateSimilarityParams, ChatQuestionQuery, RepeatedRelatedQuestion, SimilarityResponse,
  STREAM_ANSWER_KEY, STREAM_COMMENT_KEY, STREAM_IMAGE_KEY, STREAM_METADATA_KEY,
};
use shared_entity::dto::chat_dto::{ChatSettings, UpdateChatParams};
use shared_entity::response::{AppResponse, AppResponseError};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tracing::error;
use uuid::Uuid;

impl Client {
  /// Create a new chat
  pub async fn create_chat(
    &self,
    workspace_id: &Uuid,
    params: CreateChatParams,
  ) -> Result<(), AppResponseError> {
    let url = format!("{}/api/chat/{workspace_id}", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn update_chat_settings(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
    params: UpdateChatParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/settings",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_error(resp).await
  }
  pub async fn get_chat_settings(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
  ) -> Result<ChatSettings, AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/settings",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<ChatSettings>(resp).await
  }

  /// Delete a chat for given chat_id
  pub async fn delete_chat(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
  ) -> Result<(), AppResponseError> {
    let url = format!("{}/api/chat/{workspace_id}/{chat_id}", self.base_url);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;
    process_response_error(resp).await
  }

  /// Save a question message to a chat
  pub async fn create_question(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
    params: CreateChatMessageParams,
  ) -> Result<ChatMessage, AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/message/question",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_data::<ChatMessage>(resp).await
  }

  /// save an answer message to a chat
  pub async fn save_answer(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
    params: CreateAnswerMessageParams,
  ) -> Result<ChatMessage, AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/message/answer",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_data::<ChatMessage>(resp).await
  }

  pub async fn stream_answer_v2(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
    question_id: i64,
  ) -> Result<QuestionStream, AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/{question_id}/v2/answer/stream",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .timeout(Duration::from_secs(30))
      .send()
      .await
      .map_err(|err| {
        let app_err = AppError::from(err);
        if matches!(app_err, AppError::ServiceTemporaryUnavailable(_)) {
          AppError::AIServiceUnavailable(
            "AI service temporarily unavailable, please try again later".to_string(),
          )
        } else {
          app_err
        }
      })?;
    let stream = AppResponse::<serde_json::Value>::json_response_stream(resp).await?;
    Ok(QuestionStream::new(stream))
  }

  pub async fn stream_answer_v3(
    &self,
    workspace_id: &Uuid,
    query: ChatQuestionQuery,
    chat_model: Option<String>,
  ) -> Result<QuestionStream, AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{}/answer/stream",
      self.base_url, query.chat_id
    );
    let resp = self
      .http_client_with_model(Method::POST, &url, chat_model)
      .await?
      .timeout(Duration::from_secs(60))
      .json(&query)
      .send()
      .await?;
    let stream = AppResponse::<serde_json::Value>::json_response_stream(resp).await?;
    Ok(QuestionStream::new(stream))
  }

  pub async fn get_answer(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
    question_message_id: i64,
  ) -> Result<ChatMessage, AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/{question_message_id}/answer",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<ChatMessage>(resp).await
  }

  /// Update chat message content. It will override the content of the message.
  /// A message can be a question or an answer
  pub async fn update_chat_message(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
    params: UpdateChatMessageContentParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/message",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  /// Get related question for a chat message. The message_d should be the question's id
  pub async fn get_chat_related_question(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
    message_id: i64,
  ) -> Result<RepeatedRelatedQuestion, AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/{message_id}/related_question",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<RepeatedRelatedQuestion>(resp).await
  }

  /// Deprecated since v0.9.24. Return list of chat messages for a chat
  pub async fn get_chat_messages(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
    offset: MessageCursor,
    limit: u64,
  ) -> Result<RepeatedChatMessage, AppResponseError> {
    let mut url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/message",
      self.base_url
    );
    let mut query_params = vec![("limit", limit.to_string())];
    match offset {
      MessageCursor::Offset(offset_value) => {
        query_params.push(("offset", offset_value.to_string()));
      },
      MessageCursor::AfterMessageId(message_id) => {
        query_params.push(("after", message_id.to_string()));
      },
      MessageCursor::BeforeMessageId(message_id) => {
        query_params.push(("before", message_id.to_string()));
      },
      MessageCursor::NextBack => {},
    }
    let query = serde_urlencoded::to_string(&query_params).unwrap();
    url = format!("{}?{}", url, query);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<RepeatedChatMessage>(resp).await
  }

  /// Return list of chat messages for a chat. Each message will have author_uuid as
  /// as the author's uid, as author_uid will face precision issue in the browser environment.
  pub async fn get_chat_messages_with_author_uuid(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
    offset: MessageCursor,
    limit: u64,
  ) -> Result<RepeatedChatMessageWithAuthorUuid, AppResponseError> {
    let mut url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/message",
      self.base_url
    );
    let mut query_params = vec![("limit", limit.to_string())];
    match offset {
      MessageCursor::Offset(offset_value) => {
        query_params.push(("offset", offset_value.to_string()));
      },
      MessageCursor::AfterMessageId(message_id) => {
        query_params.push(("after", message_id.to_string()));
      },
      MessageCursor::BeforeMessageId(message_id) => {
        query_params.push(("before", message_id.to_string()));
      },
      MessageCursor::NextBack => {},
    }
    let query = serde_urlencoded::to_string(&query_params).unwrap();
    url = format!("{}?{}", url, query);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<RepeatedChatMessageWithAuthorUuid>(resp).await
  }

  pub async fn get_question_message_from_answer_id(
    &self,
    workspace_id: &Uuid,
    chat_id: &str,
    answer_message_id: i64,
  ) -> Result<Option<ChatMessage>, AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/message/find_question",
      self.base_url
    );

    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .query(&[("answer_message_id", answer_message_id)])
      .send()
      .await?;
    process_response_data::<Option<ChatMessage>>(resp).await
  }

  pub async fn calculate_similarity(
    &self,
    params: CalculateSimilarityParams,
  ) -> Result<SimilarityResponse, AppResponseError> {
    let url = format!(
      "{}/api/ai/{}/calculate_similarity",
      self.base_url, &params.workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_data::<SimilarityResponse>(resp).await
  }
}

#[pin_project]
pub struct QuestionStream {
  stream: Pin<Box<dyn Stream<Item = Result<serde_json::Value, AppResponseError>> + Send>>,
  buffer: Vec<u8>,
}

impl QuestionStream {
  pub fn new<S>(stream: S) -> Self
  where
    S: Stream<Item = Result<serde_json::Value, AppResponseError>> + Send + 'static,
  {
    QuestionStream {
      stream: Box::pin(stream),
      buffer: Vec::new(),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuestionStreamValue {
  Answer {
    value: String,
  },
  /// Metadata is a JSON array object. its structure as below:
  /// ```json
  /// [
  ///   {"id": "xx", "source": "", "name": "" }
  /// ]
  Metadata {
    value: Value,
  },
  SuggestedQuestion {
    context_suggested_questions: Vec<ContextSuggestedQuestion>,
  },
  FollowUp {
    should_generate_related_question: bool,
  },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSuggestedQuestion {
  pub content: String,
  pub object_id: String,
}

impl Stream for QuestionStream {
  type Item = Result<QuestionStreamValue, AppResponseError>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let this = self.project();

    match ready!(this.stream.as_mut().poll_next(cx)) {
      Some(Ok(value)) => match value {
        Value::Object(mut value) => {
          if let Some(metadata) = value.remove(STREAM_METADATA_KEY) {
            return Poll::Ready(Some(Ok(QuestionStreamValue::Metadata { value: metadata })));
          }

          if let Some(answer) = value
            .remove(STREAM_ANSWER_KEY)
            .and_then(|s| s.as_str().map(ToString::to_string))
          {
            return Poll::Ready(Some(Ok(QuestionStreamValue::Answer { value: answer })));
          }

          if let Some(image) = value
            .remove(STREAM_IMAGE_KEY)
            .and_then(|s| s.as_str().map(ToString::to_string))
          {
            return Poll::Ready(Some(Ok(QuestionStreamValue::Answer { value: image })));
          }

          error!("Invalid streaming value: {:?}", value);
          Poll::Ready(None)
        },
        _ => {
          error!("Unexpected JSON value type: {:?}", value);
          Poll::Ready(None)
        },
      },
      Some(Err(err)) => {
        error!("Error while streaming answer: {:?}", err);
        Poll::Ready(Some(Err(err)))
      },
      None => Poll::Ready(None),
    }
  }
}

#[pin_project]
pub struct CompletionStream {
  stream: Pin<Box<dyn Stream<Item = Result<serde_json::Value, AppResponseError>> + Send>>,
  buffer: Vec<u8>,
}

impl CompletionStream {
  pub fn new<S>(stream: S) -> Self
  where
    S: Stream<Item = Result<serde_json::Value, AppResponseError>> + Send + 'static,
  {
    CompletionStream {
      stream: Box::pin(stream),
      buffer: Vec::new(),
    }
  }
}

#[derive(Debug, Clone)]
pub enum CompletionStreamValue {
  Answer { value: String },
  Comment { value: String },
}
impl Stream for CompletionStream {
  type Item = Result<CompletionStreamValue, AppResponseError>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let this = self.project();
    match ready!(this.stream.as_mut().poll_next(cx)) {
      Some(Ok(value)) => match value {
        Value::Object(mut value) => {
          if let Some(answer) = value
            .remove(STREAM_ANSWER_KEY)
            .and_then(|s| s.as_str().map(ToString::to_string))
          {
            return Poll::Ready(Some(Ok(CompletionStreamValue::Answer { value: answer })));
          }

          if let Some(comment) = value
            .remove(STREAM_COMMENT_KEY)
            .and_then(|s| s.as_str().map(ToString::to_string))
          {
            return Poll::Ready(Some(Ok(CompletionStreamValue::Comment { value: comment })));
          }

          error!("Invalid streaming value: {:?}", value);
          Poll::Ready(None)
        },
        _ => {
          error!("Unexpected JSON value type: {:?}", value);
          Poll::Ready(None)
        },
      },
      Some(Err(err)) => {
        error!("Error while streaming answer: {:?}", err);
        Poll::Ready(Some(Err(err)))
      },
      None => Poll::Ready(None),
    }
  }
}
