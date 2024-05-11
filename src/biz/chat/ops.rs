use app_error::AppError;
use database::chat;
use database::chat::chat_ops::{insert_chat, insert_chat_message, select_chat_messages};
use database_entity::chat::{
  ChatAuthor, CreateChatMessageParams, CreateChatParams, GetChatMessageParams, RepeatedChatMessage,
};
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
  params: CreateChatMessageParams,
  chat_id: &str,
) -> Result<(), AppError> {
  params.validate()?;

  insert_chat_message(pg_pool, ChatAuthor::Human { uid }, chat_id, params).await?;
  Ok(())
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
