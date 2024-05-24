use anyhow::anyhow;
use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
use database::chat;
use database::chat::chat_ops::{insert_chat, insert_chat_message, select_chat_messages};
use database_entity::dto::{
  ChatAuthor, ChatMessageType, CreateChatMessageParams, CreateChatParams, GetChatMessageParams,
  QAChatMessage, RepeatedChatMessage,
};
use sqlx::PgPool;
use std::ops::DerefMut;
use tracing::trace;
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
  _uid: i64,
  params: CreateChatMessageParams,
  chat_id: &str,
  ai_client: &AppFlowyAIClient,
) -> Result<QAChatMessage, AppError> {
  params.validate()?;

  let answer_content = match params.message_type {
    ChatMessageType::System => "".to_string(),
    ChatMessageType::User => {
      let start = std::time::Instant::now();
      trace!("[Chat] sending question to AI: {}", params.content);
      let content = ai_client
        .send_question(chat_id, &params.content)
        .await
        .map(|answer| answer.content)?;
      trace!(
        "[Chat] received answer from AI: {}, cost:{} millis",
        content,
        start.elapsed().as_millis()
      );
      content
    },
  };

  let mut txn = pg_pool.begin().await.map_err(|err| {
    AppError::Internal(anyhow!(
      "failed to start transaction for inserting chat message: {}",
      err
    ))
  })?;
  let question =
    insert_chat_message(txn.deref_mut(), ChatAuthor::Human, chat_id, params.content).await?;

  let answer = match params.message_type {
    ChatMessageType::System => None,
    ChatMessageType::User => {
      Some(insert_chat_message(txn.deref_mut(), ChatAuthor::AI, chat_id, answer_content).await?)
    },
  };

  txn
    .commit()
    .await
    .map_err(|err| AppError::Internal(anyhow!("failed to insert chat message: {}", err)))?;

  Ok(QAChatMessage { question, answer })
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
