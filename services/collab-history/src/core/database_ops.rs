use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use std::ops::DerefMut;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AFSnapshotMetaRow {
  pub oid: String,
  pub snapshot: Vec<u8>,
  pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AFSnapshotStateRow {
  pub snapshot_id: Uuid,
  pub oid: String,
  pub doc_state: Vec<u8>,
  pub doc_state_version: i64,
  pub deps_snapshot_id: Option<Uuid>,
  pub created_at: DateTime<Utc>,
}

pub async fn insert_snapshot_meta<'a>(
  workspace_id: &str,
  oid: &str,
  snapshot: Vec<u8>,
  partition_key: i32,
  created_at: i64,
  mut transaction: Transaction<'a, Postgres>,
) -> Result<(), sqlx::Error> {
  sqlx::query!(
    r#"
    INSERT INTO af_snapshot_meta (oid, workspace_id, snapshot, partition_key, created_at)
    VALUES ($1, $2, $3, $4, $5)
    "#,
    oid,
    workspace_id,
    snapshot,
    partition_key,
    created_at,
  )
  .execute(transaction.deref_mut())
  .await?;
  Ok(())
}

pub async fn get_snapshot_meta_list<'a>(
  oid: &str,
  partition_key: i32,
  pool: &PgPool,
) -> Result<Vec<AFSnapshotMetaRow>, sqlx::Error> {
  let rows = sqlx::query_as!(
    AFSnapshotMetaRow,
    r#"
    SELECT oid, snapshot, created_at
    FROM af_snapshot_meta
    WHERE partition_key = $1
    "#,
    partition_key,
  )
  .fetch_all(pool)
  .await?;
  Ok(rows)
}

pub async fn insert_snapshot_state(
  workspace_id: &str,
  oid: &str,
  doc_state: Vec<u8>,
  doc_state_version: i64,
  deps_snapshot_id: Option<Uuid>,
  partition_key: i32,
  created_at: i64,
  mut transaction: Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
  sqlx::query!(
    r#"
    INSERT INTO af_snapshot_state (oid, workspace_id, doc_state, doc_state_version, deps_snapshot_id, partition_key, created_at)
    VALUES ($1, $2, $3, $4, $5, $6, $7)
    "#,
    oid,
    workspace_id,
    doc_state,
    doc_state_version,
    deps_snapshot_id,
    partition_key,
    created_at,
  )
  .execute(transaction.deref_mut())
  .await?;
  Ok(())
}

/// Retrieves the most recent snapshot from the `af_snapshot_state` table
/// that has a `created_at` timestamp greater than or equal to the specified timestamp.
///
async fn get_latest_snapshot(
  oid: &str,
  timestamp: DateTime<Utc>,
  mut transaction: Transaction<'_, Postgres>,
) -> Result<Option<AFSnapshotStateRow>, sqlx::Error> {
  let rec = sqlx::query_as!(
    AFSnapshotStateRow,
    r#"SELECT * FROM af_snapshot_state
           WHERE oid = $1 AND created_at >= $2
           ORDER BY created_at ASC
           LIMIT 1"#,
    oid,
    timestamp
  )
  .fetch_optional(transaction.deref_mut())
  .await?;
  Ok(rec)
}
