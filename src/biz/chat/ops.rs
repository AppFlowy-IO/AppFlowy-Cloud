use actix_web::web::Bytes;

use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
use async_stream::stream;
use database::chat;
use database::chat::chat_ops::{insert_chat, insert_chat_message, select_chat_messages};
use database_entity::dto::{
  ChatAuthor, ChatAuthorType, ChatMessageType, CreateChatMessageParams, CreateChatParams,
  GetChatMessageParams, RepeatedChatMessage,
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
      let question = match insert_chat_message(
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

              let answer = match insert_chat_message(&pg_pool, ChatAuthor::ai(), &chat_id, content.clone()).await {
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
