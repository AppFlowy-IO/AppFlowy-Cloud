use anyhow::{anyhow, Context};
use collab_entity::CollabType;
use database_entity::dto::{
  AFAccessLevel, AFCollabMember, AFPermission, AFSnapshotMeta, AFSnapshotMetas, CollabParams,
  QueryCollab, QueryCollabResult, RawData,
};

use crate::collab::{partition_key_from_collab_type, SNAPSHOT_PER_HOUR};
use crate::pg_row::AFSnapshotRow;
use crate::pg_row::{AFCollabMemberAccessLevelRow, AFCollabRowMeta};
use app_error::AppError;
use chrono::{Duration, Utc};
use futures_util::stream::BoxStream;

use sqlx::postgres::PgRow;
use sqlx::{Error, Executor, PgPool, Postgres, Row, Transaction};
use std::collections::HashMap;
use std::fmt::Debug;
use std::{ops::DerefMut, str::FromStr};
use tracing::{error, event, instrument};
use uuid::Uuid;

/// Inserts a new row into the `af_collab` table or updates an existing row if it matches the
/// provided `object_id`.Additionally, if the row is being inserted for the first time, a corresponding
/// entry will be added to the `af_collab_member` table.
///
/// # Arguments
///
/// * `tx` - A mutable reference to a PostgreSQL transaction.
/// * `params` - Parameters required for the insertion or update operation, encapsulated in
/// the `InsertCollabParams` struct.
///
/// # Returns
///
/// * `Ok(())` if the operation is successful.
/// * `Err(StorageError::Internal)` if there's an attempt to insert a row with an existing `object_id` but a different `workspace_id`.
/// * `Err(sqlx::Error)` for other database-related errors.
///
/// # Errors
///
/// This function will return an error if:
/// * There's a database operation failure.
/// * There's an attempt to insert a row with an existing `object_id` but a different `workspace_id`.
///

#[inline]
#[instrument(level = "trace", skip(tx, params), fields(oid=%params.object_id), err)]
pub async fn insert_into_af_collab(
  tx: &mut Transaction<'_, sqlx::Postgres>,
  uid: &i64,
  workspace_id: &str,
  params: &CollabParams,
) -> Result<(), AppError> {
  let encrypt = 0;
  let partition_key = crate::collab::partition_key_from_collab_type(&params.collab_type);
  let workspace_id = Uuid::from_str(workspace_id)?;
  let existing_workspace_id: Option<Uuid> = sqlx::query_scalar!(
    "SELECT workspace_id FROM af_collab WHERE oid = $1",
    &params.object_id
  )
  .fetch_optional(tx.deref_mut())
  .await?;
  event!(
    tracing::Level::TRACE,
    "upsert collab:{}, len:{}",
    params.object_id,
    params.encoded_collab_v1.len(),
  );

  // If the collab already exists, update the row with the new data.
  // In most cases, the workspace_id should be the same as the existing one. Comparing the workspace_id
  // is a safety check to prevent a user from inserting a row with an existing object_id but a different
  // workspace_id.
  match existing_workspace_id {
    Some(existing_workspace_id) => {
      if existing_workspace_id == workspace_id {
        sqlx::query!(
          "UPDATE af_collab \
        SET blob = $2, len = $3, partition_key = $4, encrypt = $5, owner_uid = $6 WHERE oid = $1",
          params.object_id,
          params.encoded_collab_v1,
          params.encoded_collab_v1.len() as i32,
          partition_key,
          encrypt,
          uid,
        )
        .execute(tx.deref_mut())
        .await.map_err(|err| {
          AppError::Internal(anyhow!(
            "Update af_collab failed: workspace_id:{}, uid:{}, object_id:{}, collab_type:{}. error: {:?}",
            workspace_id, uid, params.object_id, params.collab_type, err,
          ))
        })?;
      } else {
        return Err(AppError::Internal(anyhow!(
          "workspace_id is not match. expect workspace_id:{}, but receive:{}",
          existing_workspace_id,
          workspace_id
        )));
      }
    },
    None => {
      // If the collab doesn't exist, insert a new row into the `af_collab` table and add a corresponding
      // entry to the `af_collab_member` table.
      let permission_id: i32 = sqlx::query_scalar!(
        r#"
          SELECT rp.permission_id
          FROM af_role_permissions rp
          JOIN af_roles ON rp.role_id = af_roles.id
          WHERE af_roles.name = 'Owner';
        "#
      )
      .fetch_one(tx.deref_mut())
      .await?;

      sqlx::query!(
        r#"
        INSERT INTO af_collab_member (uid, oid, permission_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (uid, oid)
        DO UPDATE
          SET permission_id = excluded.permission_id;
        "#,
        uid,
        params.object_id,
        permission_id
      )
      .execute(tx.deref_mut())
      .await
      .map_err(|err| {
        AppError::Internal(anyhow!(
          "Insert af_collab_member failed: {}:{}:{}. error details:{:?}",
          uid,
          params.object_id,
          permission_id,
          err
        ))
      })?;

      sqlx::query!(
        "INSERT INTO af_collab (oid, blob, len, partition_key, encrypt, owner_uid, workspace_id)\
          VALUES ($1, $2, $3, $4, $5, $6, $7)",
        params.object_id,
        params.encoded_collab_v1,
        params.encoded_collab_v1.len() as i32,
        partition_key,
        encrypt,
        uid,
        workspace_id,
      )
      .execute(tx.deref_mut())
      .await.map_err(|err| {
        AppError::Internal(anyhow!(
          "Insert new af_collab failed: workspace_id:{}, uid:{}, object_id:{}, collab_type:{}. payload len:{} error: {:?}",
         workspace_id, uid, params.object_id, params.collab_type, params.encoded_collab_v1.len(), err,
        ))
      })?;
    },
  }

  Ok(())
}

#[inline]
pub async fn select_blob_from_af_collab<'a, E>(
  conn: E,
  collab_type: &CollabType,
  object_id: &str,
) -> Result<Vec<u8>, sqlx::Error>
where
  E: Executor<'a, Database = Postgres>,
{
  let partition_key = partition_key_from_collab_type(collab_type);
  sqlx::query_scalar!(
    r#"
        SELECT blob
        FROM af_collab
        WHERE oid = $1 AND partition_key = $2 AND deleted_at IS NULL;
        "#,
    object_id,
    partition_key,
  )
  .fetch_one(conn)
  .await
}

#[inline]
pub async fn select_collab_meta_from_af_collab<'a, E>(
  conn: E,
  object_id: &str,
  collab_type: &CollabType,
) -> Result<Option<AFCollabRowMeta>, sqlx::Error>
where
  E: Executor<'a, Database = Postgres>,
{
  let partition_key = partition_key_from_collab_type(collab_type);
  sqlx::query_as!(
    AFCollabRowMeta,
    r#"
        SELECT oid,workspace_id,deleted_at,created_at
        FROM af_collab
        WHERE oid = $1 AND partition_key = $2 AND deleted_at IS NULL;
        "#,
    object_id,
    partition_key,
  )
  .fetch_optional(conn)
  .await
}

#[inline]
pub async fn batch_select_collab_blob(
  pg_pool: &PgPool,
  queries: Vec<QueryCollab>,
) -> HashMap<String, QueryCollabResult> {
  let mut results = HashMap::new();
  let mut object_ids_by_collab_type: HashMap<CollabType, Vec<String>> = HashMap::new();
  for params in queries {
    object_ids_by_collab_type
      .entry(params.collab_type)
      .or_default()
      .push(params.object_id);
  }

  for (collab_type, mut object_ids) in object_ids_by_collab_type.into_iter() {
    let partition_key = partition_key_from_collab_type(&collab_type);
    let par_results: Result<Vec<QueryCollabData>, sqlx::Error> = sqlx::query_as!(
      QueryCollabData,
      r#"
       SELECT oid, blob
       FROM af_collab
       WHERE oid = ANY($1) AND partition_key = $2 AND deleted_at IS NULL;
    "#,
      &object_ids,
      partition_key,
    )
    .fetch_all(pg_pool)
    .await;

    match par_results {
      Ok(par_results) => {
        object_ids.retain(|oid| !par_results.iter().any(|par_result| par_result.oid == *oid));

        results.extend(par_results.into_iter().map(|par_result| {
          (
            par_result.oid,
            QueryCollabResult::Success {
              encode_collab_v1: par_result.blob,
            },
          )
        }));

        results.extend(object_ids.into_iter().map(|oid| {
          (
            oid,
            QueryCollabResult::Failed {
              error: "Record not found".to_string(),
            },
          )
        }));
      },
      Err(err) => error!("Batch get collab errors: {}", err),
    }
  }

  results
}

#[derive(Debug, sqlx::FromRow)]
struct QueryCollabData {
  oid: String,
  blob: RawData,
}

pub async fn create_snapshot(
  pg_pool: &PgPool,
  object_id: &str,
  encoded_collab_v1: &[u8],
  workspace_id: &Uuid,
) -> Result<(), sqlx::Error> {
  let encrypt = 0;

  sqlx::query!(
    r#"
        INSERT INTO af_collab_snapshot (oid, blob, len, encrypt, workspace_id)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    object_id,
    encoded_collab_v1,
    encoded_collab_v1.len() as i32,
    encrypt,
    workspace_id,
  )
  .execute(pg_pool)
  .await?;
  Ok(())
}

/// Determines whether a new snapshot should be created for the given `oid`.
///
/// This asynchronous function checks the most recent snapshot creation time for the specified `oid`.
/// It compares the creation time of the latest snapshot with the current time to decide whether a new
/// snapshot should be created, based on a predefined interval (SNAPSHOT_PER_HOUR).
///
#[inline]
pub async fn latest_snapshot_time<'a, E: Executor<'a, Database = Postgres>>(
  oid: &str,
  executor: E,
) -> Result<Option<chrono::DateTime<Utc>>, sqlx::Error> {
  let latest_snapshot_time: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
    "SELECT created_at FROM af_collab_snapshot
         WHERE oid = $1 ORDER BY created_at DESC LIMIT 1",
  )
  .bind(oid)
  .fetch_optional(executor)
  .await?;
  Ok(latest_snapshot_time)
}
#[inline]
pub async fn should_create_snapshot2<'a, E: Executor<'a, Database = Postgres>>(
  oid: &str,
  executor: E,
) -> Result<bool, sqlx::Error> {
  let hours = Utc::now() - Duration::hours(SNAPSHOT_PER_HOUR);
  let latest_snapshot_time: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
    "SELECT created_at FROM af_collab_snapshot
         WHERE oid = $1 ORDER BY created_at DESC LIMIT 1",
  )
  .bind(oid)
  .fetch_optional(executor)
  .await?;
  Ok(latest_snapshot_time.map(|t| t < hours).unwrap_or(true))
}

/// Creates a new snapshot in the `af_collab_snapshot` table and maintains the total number of snapshots
/// within a specified limit for a given object ID (`oid`).
///
/// This asynchronous function inserts a new snapshot into the database and ensures that the total number
/// of snapshots stored for the specified `oid` does not exceed the provided `snapshot_limit`. If the limit
/// is exceeded, the oldest snapshots are deleted to maintain the limit.
///
pub async fn create_snapshot_and_maintain_limit<'a>(
  mut transaction: Transaction<'a, Postgres>,
  workspace_id: &str,
  oid: &str,
  encoded_collab_v1: &[u8],
  snapshot_limit: i64,
) -> Result<AFSnapshotMeta, AppError> {
  let workspace_id = Uuid::from_str(workspace_id)?;
  let snapshot_meta = sqlx::query_as!(
    AFSnapshotMeta,
    r#"
      INSERT INTO af_collab_snapshot (oid, blob, len, encrypt, workspace_id)
      VALUES ($1, $2, $3, $4, $5)
      RETURNING sid AS snapshot_id, oid AS object_id, created_at
    "#,
    oid,
    encoded_collab_v1,
    encoded_collab_v1.len() as i64,
    0,
    workspace_id,
  )
  .fetch_one(transaction.deref_mut())
  .await?;

  // When a new snapshot is created that surpasses the preset limit, older snapshots will be deleted to maintain the limit
  sqlx::query(
    r#"
       DELETE FROM af_collab_snapshot
       WHERE oid = $1 AND sid NOT IN ( SELECT sid FROM af_collab_snapshot WHERE oid = $1 ORDER BY created_at DESC LIMIT $2)
      "#,
    )
    .bind(oid)
    .bind(snapshot_limit)
    .execute(transaction.deref_mut())
    .await?;

  transaction
    .commit()
    .await
    .context("fail to commit the transaction to insert collab snapshot")?;

  Ok(snapshot_meta)
}

#[inline]
pub async fn select_snapshot(
  pg_pool: &PgPool,
  snapshot_id: &i64,
) -> Result<Option<AFSnapshotRow>, Error> {
  let row = sqlx::query_as!(
    AFSnapshotRow,
    r#"
      SELECT * FROM af_collab_snapshot
      WHERE sid = $1 AND deleted_at IS NULL;
    "#,
    snapshot_id,
  )
  .fetch_optional(pg_pool)
  .await?;
  Ok(row)
}

/// Returns list of snapshots for given object_id in descending order of creation time.
pub async fn get_all_collab_snapshot_meta(
  pg_pool: &PgPool,
  object_id: &str,
) -> Result<AFSnapshotMetas, Error> {
  let snapshots: Vec<AFSnapshotMeta> = sqlx::query_as!(
    AFSnapshotMeta,
    r#"
    SELECT sid as "snapshot_id", oid as "object_id", created_at
    FROM af_collab_snapshot
    WHERE oid = $1 AND deleted_at IS NULL
    ORDER BY created_at DESC;
    "#,
    object_id
  )
  .fetch_all(pg_pool)
  .await?;
  Ok(AFSnapshotMetas(snapshots))
}

#[inline]
#[instrument(level = "trace", skip(txn), err)]
pub async fn upsert_collab_member_with_txn<T: AsRef<str> + Debug>(
  uid: i64,
  oid: T,
  access_level: &AFAccessLevel,
  txn: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<(), AppError> {
  let oid = oid.as_ref();
  let access_level: i32 = (*access_level).into();
  let permission_id = sqlx::query_scalar!(
    r#"
     SELECT id
     FROM af_permissions
     WHERE access_level = $1
    "#,
    access_level
  )
  .fetch_one(txn.deref_mut())
  .await
  .context("Get permission id from access level fail")?;

  sqlx::query!(
    r#"
    INSERT INTO af_collab_member (uid, oid, permission_id)
    VALUES ($1, $2, $3)
    ON CONFLICT (uid, oid)
    DO UPDATE
      SET permission_id = excluded.permission_id;
    "#,
    uid,
    oid,
    permission_id
  )
  .execute(txn.deref_mut())
  .await
  .context(format!(
    "failed to insert collab member: user:{} oid:{}",
    uid, oid
  ))?;

  Ok(())
}

#[instrument(skip(txn), err)]
#[inline]
pub async fn insert_collab_member(
  uid: i64,
  oid: &str,
  access_level: &AFAccessLevel,
  txn: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<(), AppError> {
  upsert_collab_member_with_txn(uid, oid, access_level, txn).await?;
  Ok(())
}

pub async fn delete_collab_member(
  uid: i64,
  oid: &str,
  txn: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<(), AppError> {
  sqlx::query("DELETE FROM af_collab_member WHERE uid = $1 AND oid = $2")
    .bind(uid)
    .bind(oid)
    .execute(txn.deref_mut())
    .await?;
  Ok(())
}

pub fn select_collab_member_access_level(
  pg_pool: &PgPool,
) -> BoxStream<'_, sqlx::Result<AFCollabMemberAccessLevelRow>> {
  sqlx::query_as!(
    AFCollabMemberAccessLevelRow,
    r#"
      SELECT
          uid, oid, access_level
      FROM af_collab_member
      INNER JOIN af_permissions
          ON af_collab_member.permission_id = af_permissions.id
    "#
  )
  .fetch(pg_pool)
}

#[inline]
pub async fn select_collab_members(
  oid: &str,
  pg_pool: &PgPool,
) -> Result<Vec<AFCollabMember>, AppError> {
  let members = sqlx::query(
    r#"
      SELECT af_collab_member.uid, 
             af_collab_member.oid, 
             af_permissions.id, 
             af_permissions.name, 
             af_permissions.access_level, 
             af_permissions.description
      FROM af_collab_member
      JOIN af_permissions ON af_collab_member.permission_id = af_permissions.id
      WHERE af_collab_member.oid = $1
      ORDER BY af_collab_member.created_at ASC
      "#,
  )
  .bind(oid)
  .try_map(collab_member_try_from_row)
  .fetch_all(pg_pool)
  .await?;

  Ok(members)
}

#[inline]
pub async fn select_collab_member<'a, E: Executor<'a, Database = Postgres>>(
  uid: &i64,
  oid: &str,
  executor: E,
) -> Result<AFCollabMember, AppError> {
  let row = sqlx::query(
  r#"
      SELECT af_collab_member.uid, af_collab_member.oid, af_permissions.id, af_permissions.name, af_permissions.access_level, af_permissions.description
      FROM af_collab_member
      JOIN af_permissions ON af_collab_member.permission_id = af_permissions.id
      WHERE af_collab_member.uid = $1 AND af_collab_member.oid = $2
      "#,
  )
  .bind(uid)
  .bind(oid)
  .fetch_one(executor)
  .await?;

  let member = collab_member_try_from_row(row)?;
  Ok(member)
}

fn collab_member_try_from_row(row: PgRow) -> Result<AFCollabMember, sqlx::Error> {
  let access_level = AFAccessLevel::from(row.try_get::<i32, _>(4)?);
  let permission = AFPermission {
    id: row.try_get(2)?,
    name: row.try_get(3)?,
    access_level,
    description: row.try_get(5)?,
  };

  Ok(AFCollabMember {
    uid: row.try_get(0)?,
    oid: row.try_get(1)?,
    permission,
  })
}

#[inline]
pub async fn is_collab_member_exists<'a, E: Executor<'a, Database = Postgres>>(
  uid: i64,
  oid: &str,
  executor: E,
) -> Result<bool, sqlx::Error> {
  let result = sqlx::query_scalar!(
    r#"
        SELECT EXISTS (SELECT 1 FROM af_collab_member WHERE oid = $1 AND uid = $2 LIMIT 1)
        "#,
    &oid,
    &uid,
  )
  .fetch_one(executor)
  .await;
  transform_record_not_found_error(result)
}

#[inline]
fn transform_record_not_found_error(
  result: Result<Option<bool>, sqlx::Error>,
) -> Result<bool, sqlx::Error> {
  match result {
    Ok(value) => Ok(value.unwrap_or(false)),
    Err(err) => {
      if let Error::RowNotFound = err {
        Ok(false)
      } else {
        Err(err)
      }
    },
  }
}

/// Checks for the existence of a collaboration entry in the `af_collab` table using a specified `oid`.
/// Use this method to verify if a specific collaboration object is already registered in the database.
/// For a more efficient lookup, especially in frequent checks, consider using the cached method [CollabCache::is_exist].
#[inline]
pub async fn is_collab_exists<'a, E: Executor<'a, Database = Postgres>>(
  oid: &str,
  executor: E,
) -> Result<bool, sqlx::Error> {
  let result = sqlx::query_scalar!(
    r#"
        SELECT EXISTS (SELECT 1 FROM af_collab WHERE oid = $1 LIMIT 1)
        "#,
    &oid,
  )
  .fetch_one(executor)
  .await;
  transform_record_not_found_error(result)
}
