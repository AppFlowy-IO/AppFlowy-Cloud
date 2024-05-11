use crate::pg_row::AFChatRow;
use crate::workspace::is_workspace_exist;
use anyhow::anyhow;
use app_error::AppError;
use database_entity::chat::{
  ChatMessage, CreateChatMessageParams, CreateChatParams, GetChatMessageParams,
  RepeatedChatMessage, UpdateChatParams,
};
use serde_json::json;
use sqlx::postgres::PgArguments;
use sqlx::{Arguments, Executor, Postgres, Transaction};
use std::ops::DerefMut;
use std::str::FromStr;

use uuid::Uuid;

pub async fn insert_chat(
  txn: &mut Transaction<'_, Postgres>,
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
  params: UpdateChatParams,
) -> Result<(), AppError> {
  let mut query_parts = vec!["UPDATE af_chat SET".to_string()];
  let mut args = PgArguments::default();
  let mut current_param_pos = 1; // Start counting SQL parameters from 1

  if let Some(ref name) = params.name {
    query_parts.push(format!("name = ${}", current_param_pos));
    args.add(name);
    current_param_pos += 1;
  }

  if let Some(ref rag_ids) = params.rag_ids {
    query_parts.push(format!("rag_ids = ${}", current_param_pos));
    let rag_ids_json = json!(rag_ids);
    args.add(rag_ids_json);
    current_param_pos += 1;
  }

  query_parts.push(format!("WHERE chat_id = ${}", current_param_pos));
  args.add(chat_id);

  let query = query_parts.join(", ") + ";";
  let query = sqlx::query_with(&query, args);
  query.execute(txn.deref_mut()).await?;
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
  chat_id: &str,
  params: CreateChatMessageParams,
) -> Result<(), AppError> {
  let chat_id = Uuid::from_str(chat_id)?;
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

pub async fn select_chat_messages(
  txn: &mut Transaction<'_, Postgres>,
  chat_id: &str,
  params: GetChatMessageParams,
) -> Result<RepeatedChatMessage, AppError> {
  let chat_id = Uuid::from_str(chat_id)?;
  // Get the messages in descending order of created_at timestamp and message_id. This
  // ensures that even if two messages have the same timestamp, they will still be sorted
  // consistently based on their ID.
  let messages: Vec<ChatMessage> = sqlx::query_as!(
    ChatMessage,
    r#"
     SELECT message_id, content, created_at
          FROM af_chat_messages
          WHERE chat_id = $1
          ORDER BY created_at DESC, message_id DESC
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

pub async fn get_all_chat_messages<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  chat_id: &str,
) -> Result<Vec<ChatMessage>, AppError> {
  let chat_id = Uuid::from_str(chat_id)?;
  let messages: Vec<ChatMessage> = sqlx::query_as!(
    ChatMessage,
    r#"
     SELECT message_id, content, created_at
          FROM af_chat_messages
          WHERE chat_id = $1
          ORDER BY created_at DESC
   "#,
    chat_id,
  )
  .fetch_all(executor)
  .await?;
  Ok(messages)
}
