use app_error::AppError;
use database::quick_note::{
  delete_quick_note_by_id, insert_new_quick_note, select_quick_notes_with_one_more_than_limit,
  update_quick_note_by_id,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use database_entity::dto::QuickNotes;

pub async fn create_quick_note(
  pg_pool: &PgPool,
  uid: i64,
  workspace_id: Uuid,
) -> Result<(), AppError> {
  let default_data = json!([]);
  insert_new_quick_note(pg_pool, workspace_id, uid, &default_data).await
}

pub async fn update_quick_note(
  pg_pool: &PgPool,
  quick_note_id: Uuid,
  data: &serde_json::Value,
) -> Result<(), AppError> {
  update_quick_note_by_id(pg_pool, quick_note_id, data).await
}

pub async fn delete_quick_note(pg_pool: &PgPool, quick_note_id: Uuid) -> Result<(), AppError> {
  delete_quick_note_by_id(pg_pool, quick_note_id).await
}

pub async fn list_quick_notes(
  pg_pool: &PgPool,
  uid: i64,
  workspace_id: Uuid,
  search_term: Option<String>,
  offset: Option<i32>,
  limit: Option<i32>,
) -> Result<QuickNotes, AppError> {
  let mut quick_notes_with_one_more_than_limit = select_quick_notes_with_one_more_than_limit(
    pg_pool,
    workspace_id,
    uid,
    search_term,
    offset,
    limit,
  )
  .await?;
  let has_more = if let Some(limit) = limit {
    quick_notes_with_one_more_than_limit.len() as i32 > limit
  } else {
    false
  };
  if let Some(limit) = limit {
    quick_notes_with_one_more_than_limit.truncate(limit as usize);
  }
  let quick_notes = quick_notes_with_one_more_than_limit;

  Ok(QuickNotes {
    quick_notes,
    has_more,
  })
}
