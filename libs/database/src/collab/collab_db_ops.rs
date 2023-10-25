use anyhow::Context;
use collab_entity::CollabType;
use database_entity::dto::{
  AFAccessLevel, AFCollabMember, AFCollabSnapshot, AFCollabSnapshots, AFPermission,
  BatchQueryCollab, InsertCollabParams, QueryCollabResult, RawData,
};
use database_entity::error::DatabaseError;

use sqlx::postgres::PgRow;
use sqlx::{Error, PgPool, Row, Transaction};
use std::collections::HashMap;
use std::fmt::Debug;
use std::{ops::DerefMut, str::FromStr};
use tracing::{error, event, instrument};
use uuid::Uuid;

#[inline]
pub async fn collab_exists(pg_pool: &PgPool, oid: &str) -> Result<bool, sqlx::Error> {
  let result = sqlx::query_scalar!(
    r#"
        SELECT EXISTS (SELECT 1 FROM af_collab WHERE oid = $1 LIMIT 1)
        "#,
    &oid,
  )
  .fetch_one(pg_pool)
  .await;
  transform_record_not_found_error(result)
}

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
  params: &InsertCollabParams,
) -> Result<(), DatabaseError> {
  let encrypt = 0;
  let partition_key = params.collab_type.value();
  let workspace_id = Uuid::from_str(&params.workspace_id)?;
  let existing_workspace_id: Option<Uuid> = sqlx::query_scalar!(
    "SELECT workspace_id FROM af_collab WHERE oid = $1",
    &params.object_id
  )
  .fetch_optional(tx.deref_mut())
  .await?;

  match existing_workspace_id {
    Some(existing_workspace_id) => {
      if existing_workspace_id == workspace_id {
        sqlx::query!(
          "UPDATE af_collab \
        SET blob = $2, len = $3, partition_key = $4, encrypt = $5, owner_uid = $6 WHERE oid = $1",
          params.object_id,
          params.raw_data,
          params.raw_data.len() as i32,
          partition_key,
          encrypt,
          uid,
        )
        .execute(tx.deref_mut())
        .await
        .context(format!(
          "user:{} update af_collab:{} failed",
          uid, params.object_id
        ))?;
        event!(
          tracing::Level::TRACE,
          "did update collab row:{}",
          params.object_id
        );
      } else {
        return Err(DatabaseError::Internal(anyhow::anyhow!(
          "Inserting a row with an existing object_id but different workspace_id"
        )));
      }
    },
    None => {
      // Get the permission_id of the Owner
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
      .context(format!(
        "Insert af_collab_member failed: {}:{}:{}",
        uid, params.object_id, permission_id
      ))?;

      sqlx::query!(
        "INSERT INTO af_collab (oid, blob, len, partition_key, encrypt, owner_uid, workspace_id)\
          VALUES ($1, $2, $3, $4, $5, $6, $7)",
        params.object_id,
        params.raw_data,
        params.raw_data.len() as i32,
        partition_key,
        encrypt,
        uid,
        workspace_id,
      )
      .execute(tx.deref_mut())
      .await
      .context(format!(
        "Insert new af_collab failed: {}:{}:{}",
        uid, params.object_id, params.collab_type
      ))?;

      event!(
        tracing::Level::TRACE,
        "did insert new collab row: {}:{}:{}",
        uid,
        params.object_id,
        params.workspace_id
      );
    },
  }

  Ok(())
}

#[inline]
pub async fn select_blob_from_af_collab(
  pg_pool: &PgPool,
  collab_type: &CollabType,
  object_id: &str,
) -> Result<Vec<u8>, sqlx::Error> {
  let partition_key = collab_type.value();
  sqlx::query_scalar!(
    r#"
        SELECT blob
        FROM af_collab
        WHERE oid = $1 AND partition_key = $2 AND deleted_at IS NULL;
        "#,
    object_id,
    partition_key,
  )
  .fetch_one(pg_pool)
  .await
}

#[inline]
pub async fn batch_select_collab_blob(
  pg_pool: &PgPool,
  queries: Vec<BatchQueryCollab>,
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
    let partition_key = collab_type.value();
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
              blob: par_result.blob,
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

#[inline]
pub async fn delete_collab(pg_pool: &PgPool, object_id: &str) -> Result<(), sqlx::Error> {
  sqlx::query!(
    r#"
        UPDATE af_collab
        SET deleted_at = $2
        WHERE oid = $1;
        "#,
    object_id,
    chrono::Utc::now()
  )
  .execute(pg_pool)
  .await?;
  Ok(())
}

pub async fn create_snapshot(
  pg_pool: &PgPool,
  object_id: &str,
  raw_data: &[u8],
  workspace_id: &Uuid,
) -> Result<(), sqlx::Error> {
  let encrypt = 0;

  sqlx::query!(
    r#"
        INSERT INTO af_collab_snapshot (oid, blob, len, encrypt, workspace_id)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    object_id,
    raw_data,
    raw_data.len() as i32,
    encrypt,
    workspace_id,
  )
  .execute(pg_pool)
  .await?;
  Ok(())
}

#[inline]
pub async fn get_snapshot_blob(pg_pool: &PgPool, snapshot_id: i64) -> Result<Vec<u8>, sqlx::Error> {
  let blob = sqlx::query!(
    r#"
        SELECT blob
        FROM af_collab_snapshot
        WHERE sid = $1 AND deleted_at IS NULL;
        "#,
    snapshot_id,
  )
  .fetch_one(pg_pool)
  .await?
  .blob;
  Ok(blob)
}

pub async fn get_all_snapshots(
  pg_pool: &PgPool,
  object_id: &str,
) -> Result<AFCollabSnapshots, sqlx::Error> {
  let snapshots: Vec<AFCollabSnapshot> = sqlx::query_as!(
    AFCollabSnapshot,
    r#"
        SELECT sid as "snapshot_id", oid as "object_id", created_at
        FROM af_collab_snapshot where oid = $1 AND deleted_at IS NULL;
        "#,
    object_id
  )
  .fetch_all(pg_pool)
  .await?;
  Ok(AFCollabSnapshots(snapshots))
}

#[inline]
#[instrument(skip(txn), err)]
pub async fn upsert_collab_member_with_txn<T: AsRef<str> + Debug>(
  uid: i64,
  oid: T,
  access_level: &AFAccessLevel,
  txn: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<(), DatabaseError> {
  let oid = oid.as_ref();
  let access_level: i32 = access_level.clone().into();
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

#[instrument(skip(pg_pool), err)]
#[inline]
pub async fn insert_collab_member(
  uid: i64,
  oid: &str,
  access_level: &AFAccessLevel,
  pg_pool: &PgPool,
) -> Result<(), DatabaseError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("failed to acquire a transaction to insert collab member")?;

  upsert_collab_member_with_txn(uid, oid, access_level, &mut txn).await?;

  txn
    .commit()
    .await
    .context("failed to commit the transaction to insert collab member")?;
  Ok(())
}

pub async fn delete_collab_member(
  uid: i64,
  oid: &str,
  pg_pool: &PgPool,
) -> Result<(), DatabaseError> {
  sqlx::query("DELETE FROM af_collab_member WHERE uid = $1 AND oid = $2")
    .bind(uid)
    .bind(oid)
    .execute(pg_pool)
    .await?;
  Ok(())
}

#[inline]
pub async fn select_collab_members(
  oid: &str,
  pg_pool: &PgPool,
) -> Result<Vec<AFCollabMember>, DatabaseError> {
  let members = sqlx::query(
    r#"
      SELECT af_collab_member.uid, af_collab_member.oid, af_permissions.id, af_permissions.name, af_permissions.access_level, af_permissions.description
      FROM af_collab_member
      JOIN af_permissions ON af_collab_member.permission_id = af_permissions.id
      WHERE af_collab_member.oid = $1
      "#,
  )
  .bind(oid)
  .try_map(collab_member_try_from_row)
  .fetch_all(pg_pool)
  .await?;

  Ok(members)
}

#[inline]
pub async fn select_collab_member(
  uid: &i64,
  oid: &str,
  pg_pool: &PgPool,
) -> Result<AFCollabMember, DatabaseError> {
  let member = sqlx::query(
  r#"
      SELECT af_collab_member.uid, af_collab_member.oid, af_permissions.id, af_permissions.name, af_permissions.access_level, af_permissions.description
      FROM af_collab_member
      JOIN af_permissions ON af_collab_member.permission_id = af_permissions.id
      WHERE af_collab_member.uid = $1 AND af_collab_member.oid = $2
      "#,
  )
  .bind(uid)
  .bind(oid)
  .try_map(collab_member_try_from_row)
  .fetch_one(pg_pool)
  .await?;
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
pub async fn is_collab_member_exists(
  uid: i64,
  oid: &str,
  pg_pool: &PgPool,
) -> Result<bool, sqlx::Error> {
  let result = sqlx::query_scalar!(
    r#"
        SELECT EXISTS (SELECT 1 FROM af_collab_member WHERE oid = $1 AND uid = $2 LIMIT 1)
        "#,
    &oid,
    &uid,
  )
  .fetch_one(pg_pool)
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

#[inline]
pub async fn is_collab_exists(oid: &str, pg_pool: &PgPool) -> Result<bool, sqlx::Error> {
  let result = sqlx::query_scalar!(
    r#"
        SELECT EXISTS (SELECT 1 FROM af_collab WHERE oid = $1 LIMIT 1)
        "#,
    &oid,
  )
  .fetch_one(pg_pool)
  .await;
  transform_record_not_found_error(result)
}
