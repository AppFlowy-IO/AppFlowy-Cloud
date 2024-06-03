use std::ops::DerefMut;

use collab_entity::CollabType;
use pgvector::Vector;
use sqlx::Transaction;
use uuid::Uuid;

use database_entity::dto::AFCollabEmbeddingParams;

pub async fn has_collab_embeddings(
  tx: &mut Transaction<'_, sqlx::Postgres>,
  oid: &str,
) -> Result<bool, sqlx::Error> {
  let result = sqlx::query!(
    "SELECT EXISTS(SELECT 1 FROM af_collab_embeddings WHERE oid = $1)",
    oid
  )
  .fetch_one(tx.deref_mut())
  .await?;
  Ok(result.exists.unwrap_or(false))
}

pub async fn upsert_collab_embeddings(
  tx: &mut Transaction<'_, sqlx::Postgres>,
  workspace_id: &Uuid,
  tokens_used: u32,
  records: Vec<AFCollabEmbeddingParams>,
) -> Result<(), sqlx::Error> {
  if tokens_used > 0 {
    sqlx::query(
      "UPDATE af_workspace SET index_token_usage = index_token_usage + $2 WHERE workspace_id = $1",
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
    .bind(r.fragment_id)
    .bind(r.object_id)
    .bind(r.collab_type as i32)
    .bind(r.content_type as i32)
    .bind(r.content)
    .bind(r.embedding.map(Vector::from))
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

pub async fn get_collabs_without_embeddings(
  tx: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<Vec<CollabId>, sqlx::Error> {
  let oids = sqlx::query!(
    r#"
  select c.workspace_id, c.oid, c.partition_key
  from af_collab c
  where not exists (
    select 1
    from af_collab_embeddings em
    where em.oid = c.oid and em.partition_key = 0)"# // atm. get only documents
  )
  .fetch_all(tx.deref_mut())
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
