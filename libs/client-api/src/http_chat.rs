use crate::http::log_request_id;
use crate::Client;
use bytes::Bytes;
use database_entity::dto::{
  ChatMessage, CreateAnswerMessageParams, CreateChatMessageParams, CreateChatParams, MessageCursor,
  RepeatedChatMessage, UpdateChatMessageContentParams,
};
use futures_core::Stream;
use reqwest::Method;
use shared_entity::dto::ai_dto::RepeatedRelatedQuestion;
use shared_entity::response::{AppResponse, AppResponseError};

impl Client {
  pub async fn create_chat(
    &self,
    workspace_id: &str,
    params: CreateChatParams,
  ) -> Result<(), AppResponseError> {
    let url = format!("{}/api/chat/{workspace_id}", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }
  pub async fn delete_chat(
    &self,
    workspace_id: &str,
    chat_id: &str,
  ) -> Result<(), AppResponseError> {
    let url = format!("{}/api/chat/{workspace_id}/{chat_id}", self.base_url);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn create_chat_qa_message(
    &self,
    workspace_id: &str,
    chat_id: &str,
    params: CreateChatMessageParams,
  ) -> Result<impl Stream<Item = Result<ChatMessage, AppResponseError>>, AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/message",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<ChatMessage>::json_response_stream(resp).await
  }

  pub async fn create_question(
    &self,
    workspace_id: &str,
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
    log_request_id(&resp);
    AppResponse::<ChatMessage>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn create_answer(
    &self,
    workspace_id: &str,
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
    log_request_id(&resp);
    AppResponse::<ChatMessage>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn stream_answer(
    &self,
    workspace_id: &str,
    chat_id: &str,
    message_id: i64,
  ) -> Result<impl Stream<Item = Result<Bytes, AppResponseError>>, AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/{message_id}/answer/stream",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::answer_response_stream(resp).await
  }

  /// Returns the answer to a question message.
  pub async fn get_answer(
    &self,
    workspace_id: &str,
    chat_id: &str,
    message_id: i64,
  ) -> Result<ChatMessage, AppResponseError> {
    let url = format!(
      "{}/api/chat/{workspace_id}/{chat_id}/{message_id}/answer",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<ChatMessage>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn update_chat_message(
    &self,
    workspace_id: &str,
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn get_chat_related_question(
    &self,
    workspace_id: &str,
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
    log_request_id(&resp);
    AppResponse::<RepeatedRelatedQuestion>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_chat_messages(
    &self,
    workspace_id: &str,
    chat_id: &str,
    offset: MessageCursor,
    limit: u64,
  ) -> Result<RepeatedChatMessage, AppResponseError> {
    let mut url = format!("{}/api/chat/{workspace_id}/{chat_id}", self.base_url);
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
    AppResponse::<RepeatedChatMessage>::from_response(resp)
      .await?
      .into_data()
  }
}
