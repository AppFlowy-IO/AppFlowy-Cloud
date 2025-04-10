use crate::collab::partition_key_from_collab_type;
use chrono::{DateTime, Utc};
use collab_entity::CollabType;
use database_entity::dto::{AFCollabEmbeddedChunk, IndexingStatus, QueryCollab, QueryCollabParams};
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use pgvector::Vector;
use serde_json::json;
use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgHasArrayType, PgTypeInfo};
use sqlx::{Error, Executor, Postgres, Transaction};
use std::collections::HashMap;
use std::ops::DerefMut;
use uuid::Uuid;

pub async fn get_index_status<'a, E>(
  tx: E,
  workspace_id: &Uuid,
  object_id: &Uuid,
) -> Result<IndexingStatus, sqlx::Error>
where
  E: Executor<'a, Database = Postgres>,
{
  let result = sqlx::query!(
    r#"
SELECT
  w.settings['disable_search_indexing']::boolean as disable_search_indexing,
  CASE
    WHEN w.settings['disable_search_indexing']::boolean THEN
      FALSE
    ELSE
      EXISTS (SELECT 1 FROM af_collab_embeddings m WHERE m.oid = $2::uuid)
  END as has_index
FROM af_workspace w
WHERE w.workspace_id = $1"#,
    workspace_id,
    object_id
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

#[derive(sqlx::Type)]
#[sqlx(type_name = "af_fragment_v3", no_pg_array)]
pub struct Fragment {
  pub fragment_id: String,
  pub content_type: i32,
  pub contents: Option<String>,
  pub embedding: Option<Vector>,
  pub metadata: serde_json::Value,
  pub fragment_index: i32,
  pub embedded_type: i16,
}

impl From<AFCollabEmbeddedChunk> for Fragment {
  fn from(value: AFCollabEmbeddedChunk) -> Self {
    Fragment {
      fragment_id: value.fragment_id,
      content_type: value.content_type as i32,
      contents: value.content,
      embedding: value.embedding.map(Vector::from),
      metadata: value.metadata,
      fragment_index: value.fragment_index,
      embedded_type: value.embedded_type,
    }
  }
}

impl PgHasArrayType for Fragment {
  fn array_type_info() -> PgTypeInfo {
    PgTypeInfo::with_name("af_fragment_v3[]")
  }
}

pub async fn upsert_collab_embeddings(
  transaction: &mut Transaction<'_, Postgres>,
  workspace_id: &Uuid,
  object_id: &Uuid,
  tokens_used: u32,
  chunks: Vec<AFCollabEmbeddedChunk>,
) -> Result<(), sqlx::Error> {
  let fragments = chunks.into_iter().map(Fragment::from).collect::<Vec<_>>();
  tracing::trace!(
    "[Embedding] upsert {} {} fragments, fragment ids: {:?}",
    object_id,
    fragments.len(),
    fragments
      .iter()
      .map(|v| v.fragment_id.clone())
      .collect::<Vec<_>>()
  );
  sqlx::query(r#"CALL af_collab_embeddings_upsert($1, $2, $3, $4::af_fragment_v3[])"#)
    .bind(*workspace_id)
    .bind(object_id)
    .bind(tokens_used as i32)
    .bind(fragments)
    .execute(transaction.deref_mut())
    .await?;
  Ok(())
}

pub async fn get_collab_embedding_fragment<'a, E>(
  tx: E,
  object_id: &Uuid,
) -> Result<Vec<Fragment>, sqlx::Error>
where
  E: Executor<'a, Database = Postgres>,
{
  let rows = sqlx::query!(
    r#"
    SELECT
      fragment_id,
      content_type,
      content,
      embedding as "embedding!: Option<Vector>",
      metadata,
      fragment_index,
      embedder_type
    FROM af_collab_embeddings
    WHERE oid = $1
    ORDER BY fragment_index
    "#,
    object_id
  )
  .fetch_all(tx)
  .await?;

  let fragments = rows
    .into_iter()
    .map(|row| Fragment {
      fragment_id: row.fragment_id,
      content_type: row.content_type,
      contents: row.content,
      embedding: row.embedding,
      metadata: row.metadata.unwrap_or_else(|| json!({})),
      fragment_index: row.fragment_index.unwrap_or(0),
      embedded_type: row.embedder_type.unwrap_or(0),
    })
    .collect();

  Ok(fragments)
}

pub async fn get_collab_embedding_fragment_ids<'a, E>(
  tx: E,
  collab_ids: Vec<Uuid>,
) -> Result<HashMap<Uuid, Vec<String>>, sqlx::Error>
where
  E: Executor<'a, Database = Postgres>,
{
  let records = sqlx::query!(
    r#"
        SELECT oid, fragment_id
        FROM af_collab_embeddings
        WHERE oid = ANY($1::uuid[])
        "#,
    &collab_ids,
  )
  .fetch_all(tx)
  .await?;

  let mut fragment_ids_by_oid = HashMap::new();
  for record in records {
    // If your record.oid is not a String, convert it as needed.
    fragment_ids_by_oid
      .entry(record.oid)
      .or_insert_with(Vec::new)
      .push(record.fragment_id);
  }
  Ok(fragment_ids_by_oid)
}

pub async fn stream_collabs_without_embeddings(
  conn: &mut PoolConnection<Postgres>,
  workspace_id: Uuid,
  limit: i64,
) -> BoxStream<sqlx::Result<CollabId>> {
  sqlx::query!(
    r#"
        SELECT c.workspace_id, c.oid, c.partition_key
        FROM af_collab c
        JOIN af_workspace w ON c.workspace_id = w.workspace_id
        WHERE c.workspace_id = $1
        AND NOT COALESCE(w.settings['disable_search_indexing']::boolean, false)
        AND c.indexed_at IS NULL
        ORDER BY c.updated_at DESC
        LIMIT $2
    "#,
    workspace_id,
    limit
  )
  .fetch(conn.deref_mut())
  .map(|row| {
    row.map(|r| CollabId {
      collab_type: CollabType::from(r.partition_key),
      workspace_id: r.workspace_id,
      object_id: r.oid,
    })
  })
  .boxed()
}

pub async fn update_collab_indexed_at<'a, E>(
  tx: E,
  object_id: &Uuid,
  collab_type: &CollabType,
  indexed_at: DateTime<Utc>,
) -> Result<(), Error>
where
  E: Executor<'a, Database = Postgres>,
{
  let partition_key = partition_key_from_collab_type(collab_type);
  sqlx::query!(
    r#"
      UPDATE af_collab
      SET indexed_at = $1
      WHERE oid = $2 AND partition_key = $3
    "#,
    indexed_at,
    object_id,
    partition_key
  )
  .execute(tx)
  .await?;

  Ok(())
}

pub async fn get_collabs_indexed_at<'a, E>(
  executor: E,
  oids: Vec<Uuid>,
) -> Result<HashMap<Uuid, DateTime<Utc>>, Error>
where
  E: Executor<'a, Database = Postgres>,
{
  let result = sqlx::query!(
    r#"
        SELECT oid, indexed_at
        FROM af_collab
        WHERE oid = ANY (SELECT UNNEST($1::uuid[]))
        "#,
    &oids
  )
  .fetch_all(executor)
  .await?;

  let map = result
    .into_iter()
    .filter_map(|r| r.indexed_at.map(|indexed_at| (r.oid, indexed_at)))
    .collect::<HashMap<Uuid, DateTime<Utc>>>();
  Ok(map)
}

#[derive(Debug, Clone)]
pub struct CollabId {
  pub collab_type: CollabType,
  pub workspace_id: Uuid,
  pub object_id: Uuid,
}

impl From<CollabId> for QueryCollabParams {
  fn from(value: CollabId) -> Self {
    QueryCollabParams {
      workspace_id: value.workspace_id,
      inner: QueryCollab {
        object_id: value.object_id,
        collab_type: value.collab_type,
      },
    }
  }
}
