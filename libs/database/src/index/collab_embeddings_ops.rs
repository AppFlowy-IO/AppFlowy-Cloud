use std::ops::DerefMut;

use collab_entity::CollabType;
use pgvector::Vector;
use sqlx::{Error, Executor, Postgres, Transaction};
use uuid::Uuid;

use database_entity::dto::{AFCollabEmbeddingParams, IndexingStatus};

pub async fn get_index_status<'a, E>(
  tx: E,
  workspace_id: &Uuid,
  object_id: &str,
  partition_key: i32,
) -> Result<IndexingStatus, sqlx::Error> 
where
  E: Executor<'a, Database = Postgres>, {
  let result = sqlx::query!(
    r#"
SELECT
  w.settings['disable_search_indexing']::boolean as disable_search_indexing,
  CASE
    WHEN w.settings['disable_search_indexing']::boolean THEN
      FALSE
    ELSE
      EXISTS (SELECT 1 FROM af_collab_embeddings m WHERE m.partition_key = $3 AND m.oid = $2)
  END as has_index
FROM af_workspace w
WHERE w.workspace_id = $1"#,
    workspace_id,
    object_id,
    partition_key
  )
  .fetch_one(tx)
  .await;
  match result {
    Ok(row) => {
      if row.disable_search_indexing.unwrap_or(false) {
        Ok(IndexingStatus::Disabled)
      } else if row.has_index.unwrap_or(false) {
        Ok(IndexingStatus::Indexed)
      } else {
        Ok(IndexingStatus::NotIndexed)
      }
    },
    Err(Error::RowNotFound) => {
      tracing::warn!(
        "open-collab event for {}/{} arrived before its workspace was created",
        workspace_id,
        object_id
      );
      Ok(IndexingStatus::NotIndexed)
    },
    Err(e) => Err(e),
  }
}

pub async fn upsert_collab_embeddings(
  tx: &mut Transaction<'_, sqlx::Postgres>,
  workspace_id: &Uuid,
  tokens_used: u32,
  records: &[AFCollabEmbeddingParams],
) -> Result<(), sqlx::Error> {
  if tokens_used > 0 {
    sqlx::query(r#"
      INSERT INTO af_workspace_ai_usage(created_at, workspace_id, search_requests, search_tokens_consumed, index_tokens_consumed)
      VALUES (now()::date, $1, 0, 0, $2)
      ON CONFLICT (created_at, workspace_id) DO UPDATE
      SET index_tokens_consumed = af_workspace_ai_usage.index_tokens_consumed + $2"#,
    )
    .bind(workspace_id)
    .bind(tokens_used as i64)
    .execute(tx.deref_mut())
    .await?;
  }

  for r in records {
    sqlx::query(
      r#"INSERT INTO af_collab_embeddings (fragment_id, oid, partition_key, content_type, content, embedding, indexed_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        ON CONFLICT (fragment_id) DO UPDATE SET content_type = $4, content = $5, embedding = $6, indexed_at = NOW()"#,
    )
    .bind(&r.fragment_id)
    .bind(&r.object_id)
    .bind(r.collab_type.clone() as i32)
    .bind(r.content_type as i32)
    .bind(&r.content)
    .bind(r.embedding.clone().map(Vector::from))
    .execute(tx.deref_mut())
    .await?;
  }
  Ok(())
}

pub async fn remove_collab_embeddings(
  tx: &mut Transaction<'_, sqlx::Postgres>,
  ids: &[String],
) -> Result<(), sqlx::Error> {
  sqlx::query!(
    "DELETE FROM af_collab_embeddings WHERE fragment_id IN (SELECT unnest($1::text[]))",
    ids
  )
  .execute(tx.deref_mut())
  .await?;
  Ok(())
}

pub async fn get_collabs_without_embeddings<'a, E>(
  executor: E,
) -> Result<Vec<CollabId>, sqlx::Error>
where
  E: Executor<'a, Database = Postgres>,
{
  let oids = sqlx::query!(
    r#"
  select c.workspace_id, c.oid, c.partition_key
  from af_collab c
  join af_workspace w on c.workspace_id = w.workspace_id
  where not coalesce(w.settings['disable_search_indexding']::boolean, false)
    and not exists (
    select 1
    from af_collab_embeddings em
    where em.oid = c.oid and em.partition_key = 0)"# // atm. get only documents
  )
  .fetch_all(executor)
  .await?;
  Ok(
    oids
      .into_iter()
      .map(|r| CollabId {
        collab_type: CollabType::from(r.partition_key),
        workspace_id: r.workspace_id,
        object_id: r.oid,
      })
      .collect(),
  )
}

#[derive(Debug, Clone)]
pub struct CollabId {
  pub collab_type: CollabType,
  pub workspace_id: Uuid,
  pub object_id: String,
}
