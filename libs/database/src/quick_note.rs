use app_error::AppError;
use database_entity::dto::QuickNote;
use sqlx::{Executor, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::pg_row::AFQuickNoteRow;

pub async fn insert_new_quick_note<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: Uuid,
  uid: i64,
  data: &serde_json::Value,
) -> Result<QuickNote, AppError> {
  let quick_note = sqlx::query_as!(
    QuickNote,
    r#"
      INSERT INTO af_quick_note (workspace_id, uid, data) VALUES ($1, $2, $3)
      RETURNING quick_note_id AS id, data, created_at AS "created_at!", updated_at AS "last_updated_at!"
    "#,
    workspace_id,
    uid,
    data
  )
  .fetch_one(executor)
  .await?;
  Ok(quick_note)
}

pub async fn select_quick_notes_with_one_more_than_limit<
  'a,
  E: Executor<'a, Database = Postgres>,
>(
  executor: E,
  workspace_id: Uuid,
  uid: i64,
  search_term: Option<String>,
  offset: Option<i32>,
  limit: Option<i32>,
) -> Result<Vec<QuickNote>, AppError> {
  let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
    r#"
    SELECT
      quick_note_id,
      data,
      created_at,
      updated_at
    FROM af_quick_note WHERE workspace_id =
    "#,
  );
  query_builder.push_bind(workspace_id);
  query_builder.push(" AND uid = ");
  query_builder.push_bind(uid);
  if let Some(search_term) = search_term.filter(|term| !term.is_empty()) {
    query_builder.push(" AND data @? ");
    let json_path_query = format!("'$.**.insert ? (@ like_regex \".*{}.*\")'", search_term);
    query_builder.push(json_path_query);
  }
  query_builder.push(" ORDER BY created_at DESC");
  if let Some(limit) = limit {
    query_builder.push(" LIMIT ");
    query_builder.push_bind(limit);
    query_builder.push(" + 1 ");
  }
  if let Some(offset) = offset {
    query_builder.push(" OFFSET ");
    query_builder.push_bind(offset);
  }
  let query = query_builder.build_query_as::<AFQuickNoteRow>();
  let quick_notes_with_one_more_than_limit = query
    .fetch_all(executor)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
  Ok(quick_notes_with_one_more_than_limit)
}

pub async fn update_quick_note_by_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  quick_note_id: Uuid,
  data: &serde_json::Value,
) -> Result<(), AppError> {
  sqlx::query!(
    "UPDATE af_quick_note SET data = $1, updated_at = NOW() WHERE quick_note_id = $2",
    data,
    quick_note_id
  )
  .execute(executor)
  .await?;
  Ok(())
}

pub async fn delete_quick_note_by_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  quick_note_id: Uuid,
) -> Result<(), AppError> {
  sqlx::query!(
    "DELETE FROM af_quick_note WHERE quick_note_id = $1",
    quick_note_id
  )
  .execute(executor)
  .await?;
  Ok(())
}
