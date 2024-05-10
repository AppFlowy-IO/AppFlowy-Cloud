use crate::pg_row::AFChatRow;
use crate::workspace::is_workspace_exist;
use anyhow::anyhow;
use app_error::AppError;
use database_entity::chat::{
  ChatMessage, CreateChatMessageParams, CreateChatParams, GetChatMessageParams, RepeatedChatMessage,
};
use serde_json::json;
use sqlx::{Executor, Postgres, Transaction};
use std::ops::DerefMut;
use std::str::FromStr;
use tracing::instrument;
use uuid::Uuid;

pub async fn insert_chat(
  txn: &mut Transaction<'_, Postgres>,
  uid: &i64,
  workspace_id: &str,
  params: CreateChatParams,
) -> Result<(), AppError> {
  let chat_id = Uuid::from_str(&params.chat_id)?;
  let workspace_id = Uuid::from_str(workspace_id)?;
  if !is_workspace_exist(txn.deref_mut(), &workspace_id).await? {
    return Err(AppError::RecordNotFound(format!(
      "workspace with given id:{} is not found",
      workspace_id
    )));
  }
  let rag_ids = json!(params.rag_ids);

  sqlx::query!(
    r#"
       INSERT INTO af_chat (chat_id, name, workspace_id, rag_ids)
       VALUES ($1, $2, $3, $4)
    "#,
    chat_id,
    params.name,
    workspace_id,
    rag_ids,
  )
  .execute(txn.deref_mut())
  .await
  .map_err(|err| AppError::Internal(anyhow!("Failed to insert chat: {}", err)))?;

  Ok(())
}

pub async fn update_chat(
  txn: &mut Transaction<'_, Postgres>,
  chat_id: &Uuid,
  params: CreateChatParams,
) -> Result<(), AppError> {
  let rag_ids = json!(params.rag_ids);
  sqlx::query!(
    r#"
        UPDATE af_chat
        SET name = $1, rag_ids = $2
        WHERE chat_id = $3
    "#,
    params.name,
    rag_ids,
    chat_id,
  )
  .execute(txn.deref_mut())
  .await?;
  Ok(())
}

pub async fn delete_chat(
  txn: &mut Transaction<'_, Postgres>,
  chat_id: &str,
) -> Result<(), AppError> {
  let chat_id = Uuid::from_str(chat_id)?;
  sqlx::query!(
    r#"
        UPDATE af_chat
        SET deleted_at = now()
        WHERE chat_id = $1
    "#,
    chat_id,
  )
  .execute(txn.deref_mut())
  .await?;
  Ok(())
}

pub async fn get_chat<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  chat_id: &str,
) -> Result<AFChatRow, AppError> {
  let chat_id = Uuid::from_str(chat_id)?;
  let row = sqlx::query_as!(
    AFChatRow,
    r#"
        SELECT *
        FROM af_chat
        WHERE chat_id = $1 AND deleted_at IS NULL
    "#,
    &chat_id,
  )
  .fetch_optional(executor)
  .await?;
  match row {
    Some(row) => Ok(row),
    None => Err(AppError::RecordNotFound(format!(
      "chat with given id:{} is not found",
      chat_id
    ))),
  }
}

pub async fn insert_chat_message<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  params: CreateChatMessageParams,
) -> Result<(), AppError> {
  let chat_id = Uuid::from_str(&params.chat_id)?;
  sqlx::query!(
    r#"
       INSERT INTO af_chat_messages (chat_id, content)
       VALUES ($1, $2)
    "#,
    chat_id,
    params.content,
  )
  .execute(executor)
  .await
  .map_err(|err| AppError::Internal(anyhow!("Failed to insert chat message: {}", err)))?;
  Ok(())
}

pub async fn get_chat_messages(
  txn: &mut Transaction<'_, Postgres>,
  params: GetChatMessageParams,
) -> Result<RepeatedChatMessage, AppError> {
  let chat_id = Uuid::from_str(&params.chat_id)?;
  // Get number of messages base on offset and limit. return number of ChatMessage and has_more flag
  // and the total number of messages.
  let messages: Vec<ChatMessage> = sqlx::query_as!(
    ChatMessage,
    r#"
     SELECT message_id, content, created_at
          FROM af_chat_messages
          WHERE chat_id = $1
          ORDER BY created_at DESC
          LIMIT $2 OFFSET $3
   "#,
    &chat_id,
    params.limit as i64,
    params.offset as i64,
  )
  .fetch_all(txn.deref_mut())
  .await?;

  let total = sqlx::query_scalar!(
    r#"
      SELECT COUNT(*)
      FROM public.af_chat_messages
      WHERE chat_id = $1
    "#,
    &chat_id
  )
  .fetch_one(txn.deref_mut())
  .await?
  .unwrap_or(0);
  let has_more = (params.offset + params.limit) < total as u64;

  Ok(RepeatedChatMessage {
    messages,
    total,
    has_more,
  })
}
