use std::ops::DerefMut;

use chrono::{DateTime, Utc};
use pgvector::Vector;
use sqlx::Transaction;
use uuid::Uuid;

pub async fn search_documents(
  tx: &mut Transaction<'_, sqlx::Postgres>,
  params: SearchDocumentParams,
) -> Result<Vec<SearchDocumentItem>, sqlx::Error> {
  let query = sqlx::query_as::<_, SearchDocumentItem>(
    r#"
    SELECT
      em.oid AS object_id,
      collab.workspace_id,
      em.partition_key AS collab_type,
      LEFT(em.content, 180) AS content_start,
      user.name AS created_by,
      collab.created_at AS created_at,
      em.embedding <=> $3 AS score,
    FROM af_collab_embeddings em
    JOIN af_collab collab ON em.oid = collab.oid AND em.partition_key = collab.partition_key
    JOIN af_collab_member member ON collab.oid = member.oid
    JOIN af_user user ON collab.owner_uid = user.uid
    WHERE member.uid = $1 AND collab.workspace_id = $2 AND collab.deleted_at IS NULL
    ORDER BY em.embedding <=> $3
    LIMIT $4
  "#,
  )
  .bind(params.user_id)
  .bind(params.workspace_id)
  .bind(Vector::from(params.embedding))
  .bind(params.limit as i32);
  let rows = query.fetch_all(tx.deref_mut()).await?;
  Ok(rows)
}

#[derive(Debug, Clone)]
pub struct SearchDocumentParams {
  pub user_id: i64,
  pub workspace_id: Uuid,
  pub limit: u8,
  pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SearchDocumentItem {
  pub object_id: String,
  pub workpace_id: Uuid,
  pub partition_key: i64,
  pub content_start: String,
  pub created_by: String,
  pub created_at: DateTime<Utc>,
  pub score: f32,
}
