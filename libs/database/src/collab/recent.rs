use sqlx::{Error, PgPool};
use std::ops::DerefMut;

#[derive(Debug, sqlx::FromRow)]
struct RecentCollabRow {
  recent_oid: String,
  recent_partition_key: i32,
}

pub async fn select_recent_collab_with_limit(
  pg_pool: &PgPool,
  limit: i64,
) -> Result<Vec<RecentCollabRow>, Error> {
  let recents = sqlx::query_as!(
    RecentCollabRow,
    "SELECT recent_oid, recent_partition_key FROM af_collab_recent_access ORDER BY access_time DESC LIMIT $1",
    limit
  )
  .fetch_all(pg_pool)
  .await?;

  Ok(recents)
}

// TODO(nathan): Create
/// Insert a recent collab into the database. Update the record if it already exists.
pub async fn insert_or_update_recent_collab(
  pg_pool: &PgPool,
  recent_collabs: Vec<RecentCollabRow>,
) -> Result<(), Error> {
  let mut transaction = pg_pool.begin().await?;
  for collab in recent_collabs.iter() {
    sqlx::query!(
      "INSERT INTO af_collab_recent_access (recent_oid, recent_partition_key, access_time) VALUES ($1, $2, NOW())
             ON CONFLICT (recent_oid) DO UPDATE 
             SET recent_partition_key = EXCLUDED.recent_partition_key, access_time = NOW()",
      collab.recent_oid,
      collab.recent_partition_key
    )
    .execute(transaction.deref_mut())
    .await?;
  }

  transaction.commit().await?;
  Ok(())
}
