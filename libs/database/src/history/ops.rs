use crate::collab::partition_key_from_collab_type;
use collab_entity::CollabType;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, PgPool, Postgres};
use std::ops::DerefMut;
use tonic_proto::history::{HistoryStatePb, SingleSnapshotInfoPb, SnapshotMetaPb};
use tracing::trace;
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
pub async fn insert_history<'a>(
  workspace_id: &Uuid,
  oid: &str,
  doc_state: Vec<u8>,
  doc_state_version: i32,
  deps_snapshot_id: Option<String>,
  collab_type: CollabType,
  created_at: i64,
  snapshots: Vec<SnapshotMetaPb>,
  pool: PgPool,
) -> Result<(), sqlx::Error> {
  let mut transaction = pool.begin().await?;
  let partition_key = partition_key_from_collab_type(&collab_type);
  let to_insert: Vec<SnapshotMetaPb> = snapshots
    .into_iter()
    .filter(|s| s.created_at <= created_at)
    .collect();

  trace!(
    "Inserting {} snapshots into af_snapshot_meta",
    to_insert.len()
  );
  for snapshot in to_insert {
    insert_snapshot_meta(
      workspace_id,
      oid,
      snapshot,
      partition_key,
      transaction.deref_mut(),
    )
    .await?;
  }

  insert_snapshot_state(
    workspace_id,
    oid,
    doc_state,
    doc_state_version,
    deps_snapshot_id,
    partition_key,
    created_at,
    transaction.deref_mut(),
  )
  .await?;

  transaction.commit().await?;

  Ok(())
}

async fn insert_snapshot_meta<'a, E: Executor<'a, Database = Postgres>>(
  workspace_id: &Uuid,
  oid: &str,
  meta: SnapshotMetaPb,
  partition_key: i32,
  executor: E,
) -> Result<(), sqlx::Error> {
  sqlx::query!(
    r#"
    INSERT INTO af_snapshot_meta (oid, workspace_id, snapshot, snapshot_version, partition_key, created_at)
    VALUES ($1, $2, $3, $4, $5, $6)
    ON CONFLICT DO NOTHING
    "#,
    oid,
    workspace_id,
    meta.snapshot,
    meta.snapshot_version,
    partition_key,
    meta.created_at,
  )
 .execute(executor)
 .await?;
  Ok(())
}
/// Retrieves a list of snapshot metadata from the `af_snapshot_meta` table
/// sorted by the `created_at` timestamp in descending order.
///
/// # Parameters
/// - `oid`: The object identifier used to filter the results.
/// - `partition_key`: The partition key used to further filter the results.
/// - `pool`: The PostgreSQL connection pool.
///
/// # Returns
/// Returns a vector of `AFSnapshotMetaPbRow` struct instances containing the snapshot data.
/// This vector is empty if no records match the criteria.
pub async fn get_snapshot_meta_list<'a>(
  oid: &str,
  collab_type: &CollabType,
  pool: &PgPool,
) -> Result<Vec<AFSnapshotMetaPbRow>, sqlx::Error> {
  let partition_key = partition_key_from_collab_type(collab_type);
  let order_clause = "DESC";
  let query = format!(
        "SELECT oid, snapshot, snapshot_version, created_at FROM af_snapshot_meta WHERE oid = $1 AND partition_key = $2 ORDER BY created_at {}",
        order_clause
    );

  let rows = sqlx::query_as::<_, AFSnapshotMetaPbRow>(&query)
    .bind(oid)
    .bind(partition_key)
    .fetch_all(pool)
    .await?;

  Ok(rows)
}

/// Inserts a new record into the `af_snapshot_state` table.
///
/// # Parameters
/// - `workspace_id`: UUID of the workspace.
/// - `oid`: Object identifier.
/// - `doc_state`: Byte array representing the document state.
/// - `doc_state_version`: Version number of the document state.
/// - `deps_snapshot_id`: Optional UUID of a dependent snapshot.
/// - `partition_key`: Integer representing the partition where this record should be stored.
/// - `created_at`: Timestamp when the snapshot was created.
/// - `executor`: SQLx executor which could be a transaction or direct database connection.
///
#[allow(clippy::too_many_arguments)]
async fn insert_snapshot_state<'a, E: Executor<'a, Database = Postgres>>(
  workspace_id: &Uuid,
  oid: &str,
  doc_state: Vec<u8>,
  doc_state_version: i32,
  deps_snapshot_id: Option<String>,
  partition_key: i32,
  created_at: i64,
  executor: E,
) -> Result<(), sqlx::Error> {
  let deps_snapshot_id = match deps_snapshot_id {
    Some(id) => Uuid::parse_str(&id).ok(),
    None => None,
  };
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
  .execute(executor)
  .await?;
  Ok(())
}

/// Retrieves the most recent snapshot from the `af_snapshot_state` table
/// that has a `created_at` timestamp greater than or equal to the specified timestamp.
///
pub async fn get_latest_snapshot_state<'a, E: Executor<'a, Database = Postgres>>(
  oid: &str,
  timestamp: i64,
  collab_type: &CollabType,
  executor: E,
) -> Result<Option<AFSnapshotStateRow>, sqlx::Error> {
  let partition_key = partition_key_from_collab_type(collab_type);
  let rec = sqlx::query_as!(
    AFSnapshotStateRow,
    r#"
        SELECT snapshot_id, oid, doc_state, doc_state_version, deps_snapshot_id, created_at
        FROM af_snapshot_state
        WHERE oid = $1 AND partition_key = $2 AND created_at >= $3
        ORDER BY created_at ASC
        LIMIT 1
        "#,
    oid,
    partition_key,
    timestamp,
  )
  .fetch_optional(executor)
  .await?;
  Ok(rec)
}

/// Gets the latest snapshot for the specified object identifier and partition key.
pub async fn get_latest_snapshot(
  oid: &str,
  collab_type: &CollabType,
  pool: &PgPool,
) -> Result<Option<SingleSnapshotInfoPb>, sqlx::Error> {
  let mut transaction = pool.begin().await?;
  let partition_key = partition_key_from_collab_type(collab_type);
  // Attempt to fetch the latest snapshot metadata
  let snapshot_meta = sqlx::query_as!(
    AFSnapshotMetaPbRow,
    r#"
        SELECT oid, snapshot, snapshot_version, created_at
        FROM af_snapshot_meta
        WHERE oid = $1 AND partition_key = $2
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    oid,
    partition_key,
  )
  .fetch_optional(transaction.deref_mut())
  .await?;

  // Return None if no metadata found
  let snapshot_meta = match snapshot_meta {
    Some(meta) => SnapshotMetaPb {
      oid: meta.oid,
      snapshot: meta.snapshot,
      snapshot_version: meta.snapshot_version,
      created_at: meta.created_at,
    },
    None => return Ok(None),
  };

  // Fetch the corresponding state using the metadata's created_at timestamp
  // Return None if no metadata found
  let snapshot_state = match get_latest_snapshot_state(
    oid,
    snapshot_meta.created_at,
    collab_type,
    transaction.deref_mut(),
  )
  .await?
  {
    Some(state) => state,
    None => return Ok(None),
  };

  let history_state = HistoryStatePb {
    object_id: snapshot_state.oid,
    doc_state: snapshot_state.doc_state,
    doc_state_version: snapshot_state.doc_state_version,
  };

  let snapshot_info = SingleSnapshotInfoPb {
    snapshot_meta: Some(snapshot_meta),
    history_state: Some(history_state),
  };

  Ok(Some(snapshot_info))
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AFSnapshotMetaPbRow {
  pub oid: String,
  pub snapshot: Vec<u8>,
  pub snapshot_version: i32,
  pub created_at: i64,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AFSnapshotStateRow {
  pub snapshot_id: Uuid,
  pub oid: String,
  pub doc_state: Vec<u8>,
  pub doc_state_version: i32,
  pub deps_snapshot_id: Option<Uuid>,
  pub created_at: i64,
}
