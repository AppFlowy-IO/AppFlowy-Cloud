use actix_web::web::Bytes;
use anyhow::anyhow;

use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
use async_stream::stream;
use database::chat;
use database::chat::chat_ops::{
  delete_answer_message_by_question_message_id, insert_answer_message,
  insert_answer_message_with_transaction, insert_chat, insert_question_message,
  select_chat_messages,
};
use database_entity::dto::{
  ChatAuthor, ChatAuthorType, ChatMessage, ChatMessageType, CreateChatMessageParams,
  CreateChatParams, GetChatMessageParams, RepeatedChatMessage, UpdateChatMessageContentParams,
};
use futures::stream::Stream;
use sqlx::PgPool;

use validator::Validate;

pub(crate) async fn create_chat(
  pg_pool: &PgPool,
  params: CreateChatParams,
  workspace_id: &str,
) -> Result<(), AppError> {
  params.validate()?;

  let mut txn = pg_pool.begin().await?;
  insert_chat(&mut txn, workspace_id, params).await?;
  txn.commit().await?;
  Ok(())
}

pub(crate) async fn delete_chat(pg_pool: &PgPool, chat_id: &str) -> Result<(), AppError> {
  let mut txn = pg_pool.begin().await?;
  chat::chat_ops::delete_chat(&mut txn, chat_id).await?;
  txn.commit().await?;
  Ok(())
}

pub async fn update_chat_message(
  pg_pool: &PgPool,
  params: UpdateChatMessageContentParams,
  ai_client: AppFlowyAIClient,
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

  let new_answer = ai_client
    .send_question(&params.chat_id, &params.content)
    .await?;
  let _answer = insert_answer_message(
    pg_pool,
    ChatAuthor::ai(),
    &params.chat_id,
    new_answer.content,
    params.message_id,
  )
  .await?;

  Ok(())
}

pub async fn generate_chat_message_answer(
  pg_pool: &PgPool,
  ai_client: AppFlowyAIClient,
  question_message_id: i64,
  chat_id: &str,
) -> Result<ChatMessage, AppError> {
  let content = chat::chat_ops::select_chat_message_content(pg_pool, question_message_id).await?;
  let new_answer = ai_client.send_question(chat_id, &content).await?;

  // Save the answer to the database
  let mut txn = pg_pool.begin().await?;
  let message = insert_answer_message_with_transaction(
    &mut txn,
    ChatAuthor::ai(),
    chat_id,
    new_answer.content,
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
  ai_client: AppFlowyAIClient,
) -> impl Stream<Item = Result<Bytes, AppError>> {
  let params = params.clone();
  let chat_id = chat_id.clone();
  let pg_pool = pg_pool.clone();
  let stream = stream! {
      // Insert question message
      let question = match insert_question_message(
          &pg_pool,
          ChatAuthor::new(uid, ChatAuthorType::Human),
          &chat_id,
          params.content.clone()
      ).await {
          Ok(question) => question,
          Err(err) => {
              yield Err(err);
              return;
          }
      };

      let question_id = question.message_id;
      let question_bytes = match serde_json::to_vec(&question) {
          Ok(bytes) => bytes,
          Err(err) => {
              yield Err(AppError::from(err));
              return;
          }
      };

      yield Ok::<Bytes, AppError>(Bytes::from(question_bytes));

      // Insert answer message
      match params.message_type {
          ChatMessageType::System => {}
          ChatMessageType::User => {
              let content = match ai_client.send_question(&chat_id, &params.content).await {
                  Ok(response) => response.content,
                  Err(err) => {
                      yield Err(AppError::from(err));
                      return;
                  }
              };

              let answer = match insert_answer_message(&pg_pool, ChatAuthor::ai(), &chat_id, content.clone(),question_id).await {
                  Ok(answer) => answer,
                  Err(err) => {
                      yield Err(err);
                      return;
                  }
              };

              let answer_bytes = match serde_json::to_vec(&answer) {
                  Ok(bytes) => bytes,
                  Err(err) => {
                      yield Err(AppError::from(err));
                      return;
                  }
              };

              yield Ok::<Bytes, AppError>(Bytes::from(answer_bytes));
          }
      }
  };

  stream
}

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
