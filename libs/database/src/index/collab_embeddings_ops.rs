use collab_entity::CollabType;
use database_entity::dto::{AFCollabEmbeddedChunk, IndexingStatus, QueryCollab, QueryCollabParams};
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use pgvector::Vector;
use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgHasArrayType, PgTypeInfo};
use sqlx::{Error, Executor, Postgres, Transaction};
use std::ops::DerefMut;
use uuid::Uuid;

pub async fn get_index_status<'a, E>(
  tx: E,
  workspace_id: &Uuid,
  object_id: &str,
  partition_key: i32,
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

#[derive(sqlx::Type)]
#[sqlx(type_name = "af_fragment_v2", no_pg_array)]
struct Fragment {
  fragment_id: String,
  content_type: i32,
  contents: String,
  embedding: Option<Vector>,
  metadata: serde_json::Value,
}

impl From<AFCollabEmbeddedChunk> for Fragment {
  fn from(value: AFCollabEmbeddedChunk) -> Self {
    Fragment {
      fragment_id: value.fragment_id,
      content_type: value.content_type as i32,
      contents: value.content,
      embedding: value.embedding.map(Vector::from),
      metadata: value.metadata,
    }
  }
}

impl PgHasArrayType for Fragment {
  fn array_type_info() -> PgTypeInfo {
    PgTypeInfo::with_name("af_fragment_v2[]")
  }
}

pub async fn upsert_collab_embeddings(
  transaction: &mut Transaction<'_, Postgres>,
  workspace_id: &Uuid,
  tokens_used: u32,
  records: Vec<AFCollabEmbeddedChunk>,
) -> Result<(), sqlx::Error> {
  if records.is_empty() {
    return Ok(());
  }

  let object_id = records[0].object_id.clone();
  let collab_type = records[0].collab_type.clone();
  let fragments = records.into_iter().map(Fragment::from).collect::<Vec<_>>();

  sqlx::query(r#"CALL af_collab_embeddings_upsert($1, $2, $3, $4, $5::af_fragment_v2[])"#)
    .bind(*workspace_id)
    .bind(object_id)
    .bind(crate::collab::partition_key_from_collab_type(&collab_type))
    .bind(tokens_used as i32)
    .bind(fragments)
    .execute(transaction.deref_mut())
    .await?;
  Ok(())
}

pub async fn get_collabs_without_embeddings(
  conn: &mut PoolConnection<Postgres>,
  limit: i64,
) -> BoxStream<sqlx::Result<CollabId>> {
  // atm. get only documents

  sqlx::query!(
    r#"
      select c.workspace_id, c.oid, c.partition_key
      from af_collab c
      join af_workspace w on c.workspace_id = w.workspace_id
      where not coalesce(w.settings['disable_search_indexding']::boolean, false)
        and not exists (
          select 1 from af_collab_embeddings em
          where em.oid = c.oid and em.partition_key = 0
        )
      order by c.updated_at desc
      limit $1
    "#,
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

#[derive(Debug, Clone)]
pub struct CollabId {
  pub collab_type: CollabType,
  pub workspace_id: Uuid,
  pub object_id: String,
}

impl From<CollabId> for QueryCollabParams {
  fn from(value: CollabId) -> Self {
    QueryCollabParams {
      workspace_id: value.workspace_id.to_string(),
      inner: QueryCollab {
        object_id: value.object_id,
        collab_type: value.collab_type,
      },
    }
  }
}
