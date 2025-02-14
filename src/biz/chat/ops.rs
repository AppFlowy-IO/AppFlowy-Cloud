use actix_web::web::Bytes;
use anyhow::anyhow;

use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
use async_stream::stream;
use database::chat;
use database::chat::chat_ops::{
  delete_answer_message_by_question_message_id, insert_answer_message,
  insert_answer_message_with_transaction, insert_chat, insert_question_message,
  select_chat_message_matching_reply_message_id, select_chat_messages,
  select_chat_messages_with_author_uuid,
};
use futures::stream::Stream;
use serde_json::json;
use shared_entity::dto::chat_dto::{
  ChatAuthor, ChatAuthorType, ChatMessage, ChatMessageType, CreateChatMessageParams,
  CreateChatParams, GetChatMessageParams, RepeatedChatMessage, RepeatedChatMessageWithAuthorUuid,
  UpdateChatMessageContentParams,
};
use sqlx::PgPool;
use tracing::{error, info, trace};

use validator::Validate;

pub(crate) async fn create_chat(
  pg_pool: &PgPool,
  params: CreateChatParams,
  workspace_id: &str,
) -> Result<(), AppError> {
  params.validate()?;
  trace!("[Chat] create chat {:?}", params);

  insert_chat(pg_pool, workspace_id, params).await?;
  Ok(())
}

pub(crate) async fn delete_chat(pg_pool: &PgPool, chat_id: &str) -> Result<(), AppError> {
  let mut txn = pg_pool.begin().await?;
  chat::chat_ops::delete_chat(&mut txn, chat_id).await?;
  txn.commit().await?;
  Ok(())
}

pub async fn update_chat_message(
  workspace_id: String,
  pg_pool: &PgPool,
  params: UpdateChatMessageContentParams,
  ai_client: AppFlowyAIClient,
  ai_model: &str,
) -> Result<(), AppError> {
  let mut txn = pg_pool.begin().await?;
  delete_answer_message_by_question_message_id(&mut txn, params.message_id).await?;
  chat::chat_ops::update_chat_message_content(&mut txn, &params).await?;
  txn.commit().await.map_err(|err| {
    AppError::Internal(anyhow!(
      "Failed to commit transaction to update chat message: {}",
      err
    ))
  })?;

  // TODO(nathan): query the metadata from the database
  let new_answer = ai_client
    .send_question(
      &workspace_id,
      &params.chat_id,
      params.message_id,
      &params.content,
      ai_model,
      None,
    )
    .await?;
  let _answer = insert_answer_message(
    pg_pool,
    ChatAuthor::ai(),
    &params.chat_id,
    new_answer.content,
    new_answer.metadata,
    params.message_id,
  )
  .await?;

  Ok(())
}

pub async fn generate_chat_message_answer(
  workspace_id: String,
  pg_pool: &PgPool,
  ai_client: AppFlowyAIClient,
  question_message_id: i64,
  chat_id: &str,
  ai_model: &str,
) -> Result<ChatMessage, AppError> {
  let (content, metadata) =
    chat::chat_ops::select_chat_message_content(pg_pool, question_message_id).await?;
  let new_answer = ai_client
    .send_question(
      &workspace_id,
      chat_id,
      question_message_id,
      &content,
      ai_model,
      Some(metadata),
    )
    .await
    .map_err(|err| AppError::AIServiceUnavailable(err.to_string()))?;

  info!("new_answer: {:?}", new_answer);
  // Save the answer to the database
  let mut txn = pg_pool.begin().await?;
  let message = insert_answer_message_with_transaction(
    &mut txn,
    ChatAuthor::ai(),
    chat_id,
    new_answer.content,
    new_answer.metadata.unwrap_or_default(),
    question_message_id,
  )
  .await?;
  txn.commit().await.map_err(|err| {
    AppError::Internal(anyhow!(
      "Failed to commit transaction to update chat message: {}",
      err
    ))
  })?;

  Ok(message)
}

pub async fn create_chat_message(
  pg_pool: &PgPool,
  uid: i64,
  chat_id: String,
  params: CreateChatMessageParams,
) -> Result<ChatMessage, AppError> {
  let chat_id = chat_id.clone();
  let pg_pool = pg_pool.clone();

  let question = insert_question_message(
    &pg_pool,
    ChatAuthor::new(uid, ChatAuthorType::Human),
    &chat_id,
    params.content,
    params.metadata,
  )
  .await?;
  Ok(question)
}

pub async fn create_chat_message_stream(
  pg_pool: &PgPool,
  uid: i64,
  workspace_id: String,
  chat_id: String,
  params: CreateChatMessageParams,
  ai_client: AppFlowyAIClient,
  ai_model: &str,
) -> impl Stream<Item = Result<Bytes, AppError>> {
  let ai_model = ai_model.to_string();
  let params = params.clone();
  let chat_id = chat_id.clone();
  let pg_pool = pg_pool.clone();
  let stream = stream! {
      // Insert question message
      let question = match insert_question_message(
          &pg_pool,
          ChatAuthor::new(uid, ChatAuthorType::Human),
          &chat_id,
          params.content.clone(),
          params.metadata.clone(),
      ).await {
          Ok(question) => question,
          Err(err) => {
              error!("Failed to insert question message: {}", err);
              yield Err(err);
              return;
          }
      };

      let question_id = question.message_id;
      let question_bytes = match serde_json::to_vec(&question) {
          Ok(s) => Bytes::from(s),
          Err(err) => {
              error!("Failed to serialize question message: {}", err);
              yield Err(AppError::from(err));
              return;
          }
      };

      yield Ok::<Bytes, AppError>(question_bytes);

      // Insert answer message
      match params.message_type {
          ChatMessageType::System => {}
          ChatMessageType::User => {
              let answer = match ai_client.send_question(&workspace_id, &chat_id,question_id, &params.content, &ai_model, Some(json!(params.metadata))).await {
                  Ok(response) => response,
                  Err(err) => {
                      error!("Failed to send question to AI: {}", err);
                      yield Err(AppError::from(err));
                      return;
                  }
              };

              let answer = match insert_answer_message(&pg_pool, ChatAuthor::ai(), &chat_id, answer.content, answer.metadata,question_id).await {
                  Ok(answer) => answer,
                  Err(err) => {
                      error!("Failed to insert answer message: {}", err);
                      yield Err(err);
                      return;
                  }
              };

              let answer_bytes = match serde_json::to_vec(&answer) {
                  Ok(s) => Bytes::from(s),
                  Err(err) => {
                      error!("Failed to serialize answer message: {}", err);
                      yield Err(AppError::from(err));
                      return;
                  }
              };

              yield Ok::<Bytes, AppError>(answer_bytes);
          }
      }
  };

  stream
}

// Deprecated since v0.9.24
pub async fn get_chat_messages(
  pg_pool: &PgPool,
  params: GetChatMessageParams,
  chat_id: &str,
) -> Result<RepeatedChatMessage, AppError> {
  params.validate()?;

  let mut txn = pg_pool.begin().await?;
  let messages = select_chat_messages(&mut txn, chat_id, params).await?;
  txn.commit().await?;
  Ok(messages)
}

pub async fn get_chat_messages_with_author_uuid(
  pg_pool: &PgPool,
  params: GetChatMessageParams,
  chat_id: &str,
) -> Result<RepeatedChatMessageWithAuthorUuid, AppError> {
  params.validate()?;

  let mut txn = pg_pool.begin().await?;
  let messages = select_chat_messages_with_author_uuid(&mut txn, chat_id, params).await?;
  txn.commit().await?;
  Ok(messages)
}

pub async fn get_question_message(
  pg_pool: &PgPool,
  chat_id: &str,
  answer_message_id: i64,
) -> Result<Option<ChatMessage>, AppError> {
  let mut txn = pg_pool.begin().await?;
  let message =
    select_chat_message_matching_reply_message_id(&mut txn, chat_id, answer_message_id).await?;
  txn.commit().await?;
  Ok(message)
}
