use std::ops::DerefMut;

use pgvector::Vector;
use sqlx::Transaction;

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
  records: Vec<AFCollabEmbeddingParams>,
) -> Result<(), sqlx::Error> {
  for r in records {
    sqlx::query(
      r#"INSERT INTO af_collab_embeddings (fragment_id, oid, partition_key, content, embedding)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (fragment_id) DO UPDATE SET content = $4, embedding = $5"#,
    )
    .bind(r.fragment_id)
    .bind(r.object_id)
    .bind(r.collab_type.clone() as i32)
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
