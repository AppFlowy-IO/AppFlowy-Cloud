use crate::pg_row::AFChatRow;
use crate::workspace::is_workspace_exist;
use anyhow::anyhow;
use app_error::AppError;
use chrono::{DateTime, Utc};
use database_entity::dto::{
  ChatAuthor, ChatMessage, CreateChatParams, GetChatMessageParams, MessageCursor,
  RepeatedChatMessage, UpdateChatMessageParams, UpdateChatParams,
};

use serde_json::json;
use sqlx::postgres::PgArguments;
use sqlx::{Arguments, Executor, PgPool, Postgres, Transaction};
use std::ops::DerefMut;
use std::str::FromStr;
use tracing::warn;

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

/// Updates specific fields of a chat record in the database using transactional queries.
///
/// This function dynamically builds an SQL `UPDATE` query based on the provided parameters to
/// update fields of a specific chat record identified by `chat_id`. It uses a transaction to ensure
/// that the update operation is atomic.
///
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

pub async fn select_chat<'a, E: Executor<'a, Database = Postgres>>(
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
  author: ChatAuthor,
  chat_id: &str,
  content: String,
) -> Result<ChatMessage, AppError> {
  let chat_id = Uuid::from_str(chat_id)?;
  let row = sqlx::query!(
    r#"
        INSERT INTO af_chat_messages (chat_id, author, content)
        VALUES ($1, $2, $3)
        RETURNING message_id, created_at
        "#,
    chat_id,
    json!(author),
    &content,
  )
  .fetch_one(executor)
  .await
  .map_err(|err| AppError::Internal(anyhow!("Failed to insert chat message: {}", err)))?;

  let chat_message = ChatMessage {
    author,
    message_id: row.message_id,
    content,
    created_at: row.created_at,
    meta_data: Default::default(),
  };
  Ok(chat_message)
}

pub async fn select_chat_messages(
  txn: &mut Transaction<'_, Postgres>,
  chat_id: &str,
  params: GetChatMessageParams,
) -> Result<RepeatedChatMessage, AppError> {
  let chat_id = Uuid::from_str(chat_id)?;
  let mut query = r#"
        SELECT message_id, content, created_at, author, meta_data
        FROM af_chat_messages
        WHERE chat_id = $1
    "#
  .to_string();

  let mut args = PgArguments::default();
  args.add(&chat_id);

  // Message IDs:   1    2    3    4    5
  // AfterMessageId(3, 5):   [4]  [5]  has_more = false
  // BeforeMessageId(3, 5):  [1]  [2]  has_more = false
  // Offset(3, 5):           [4]  [5]  has_more = true
  match params.cursor {
    MessageCursor::AfterMessageId(after_message_id) => {
      query += " AND message_id > $2";
      args.add(after_message_id);
      query += " ORDER BY message_id DESC LIMIT $3";
      args.add(params.limit as i64);
    },
    MessageCursor::Offset(offset) => {
      query += " ORDER BY message_id ASC LIMIT $2 OFFSET $3";
      args.add(params.limit as i64);
      args.add(offset as i64);
    },
    MessageCursor::BeforeMessageId(before_message_id) => {
      query += " AND message_id < $2";
      args.add(before_message_id);
      query += " ORDER BY message_id DESC LIMIT $3";
      args.add(params.limit as i64);
    },
    MessageCursor::NextBack => {
      query += " ORDER BY message_id DESC LIMIT $2";
      args.add(params.limit as i64);
    },
  }

  let rows: Vec<(
    i64,
    String,
    DateTime<Utc>,
    serde_json::Value,
    serde_json::Value,
  )> = sqlx::query_as_with(&query, args)
    .fetch_all(txn.deref_mut())
    .await?;

  let messages = rows
    .into_iter()
    .flat_map(|(message_id, content, created_at, author, meta_data)| {
      match serde_json::from_value::<ChatAuthor>(author) {
        Ok(author) => Some(ChatMessage {
          author,
          message_id,
          content,
          created_at,
          meta_data,
        }),
        Err(err) => {
          warn!("Failed to deserialize author: {}", err);
          None
        },
      }
    })
    .collect::<Vec<ChatMessage>>();

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

  let has_more = match params.cursor {
    MessageCursor::AfterMessageId(_) => {
      if messages.is_empty() {
        false
      } else {
        sqlx::query!(
          "SELECT EXISTS(SELECT 1 FROM af_chat_messages WHERE chat_id = $1 AND message_id > $2)",
          &chat_id,
          messages[0].message_id
        )
        .fetch_one(txn.deref_mut())
        .await?
        .exists
        .unwrap_or(false)
      }
    },
    MessageCursor::Offset(offset) => (offset + params.limit) < total as u64,
    MessageCursor::BeforeMessageId(_) => {
      if messages.is_empty() {
        false
      } else {
        sqlx::query!(
          "SELECT EXISTS(SELECT 1 FROM af_chat_messages WHERE chat_id = $1 AND message_id < $2)",
          &chat_id,
          messages.last().as_ref().unwrap().message_id
        )
        .fetch_one(txn.deref_mut())
        .await?
        .exists
        .unwrap_or(false)
      }
    },
    MessageCursor::NextBack => params.limit < total as u64,
  };

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
  let rows = sqlx::query!(
    // ChatMessage,
    r#"
     SELECT message_id, content, created_at, author, meta_data
          FROM af_chat_messages
          WHERE chat_id = $1
          ORDER BY created_at ASC
   "#,
    chat_id,
  )
  .fetch_all(executor)
  .await?;

  let messages = rows
    .into_iter()
    .flat_map(
      |row| match serde_json::from_value::<ChatAuthor>(row.author) {
        Ok(author) => Some(ChatMessage {
          author,
          message_id: row.message_id,
          content: row.content,
          created_at: row.created_at,
          meta_data: row.meta_data,
        }),
        Err(err) => {
          warn!("Failed to deserialize author: {}", err);
          None
        },
      },
    )
    .collect::<Vec<ChatMessage>>();

  Ok(messages)
}

pub async fn update_chat_message(
  pg_pool: &PgPool,
  params: UpdateChatMessageParams,
) -> Result<(), AppError> {
  for (key, value) in params.meta_data.iter() {
    sqlx::query(
      r#"
           UPDATE af_chat_messages
           SET meta_data = jsonb_set(
               COALESCE(meta_data, '{}'),
               $2,
               $3::jsonb,
               true
           )
           WHERE id = $1
           "#,
    )
    .bind(params.message_id)
    .bind(format!("{{{}}}", key))
    .bind(value)
    .execute(pg_pool)
    .await?;
  }

  Ok(())
}
