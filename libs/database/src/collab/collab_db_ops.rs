use anyhow::Context;
use collab_entity::CollabType;
use database_entity::{
  database_error::DatabaseError, AFCollabSnapshot, AFCollabSnapshots, InsertCollabParams,
  QueryCollabParams, QueryCollabResult, RawData,
};

use sqlx::{PgPool, Transaction};
use std::collections::HashMap;
use std::{ops::DerefMut, str::FromStr};
use tracing::{error, trace};
use uuid::Uuid;

pub async fn collab_exists(pg_pool: &PgPool, oid: &str) -> Result<bool, sqlx::Error> {
  sqlx::query_scalar!(
    r#"
        SELECT EXISTS (SELECT 1 FROM af_collab WHERE oid = $1 LIMIT 1)
        "#,
    &oid,
  )
  .fetch_one(pg_pool)
  .await?
  .ok_or(sqlx::Error::RowNotFound)
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
pub async fn insert_af_collab(
  tx: &mut Transaction<'_, sqlx::Postgres>,
  owner_uid: i64,
  params: &InsertCollabParams,
) -> Result<(), DatabaseError> {
  let encrypt = 0;
  let partition_key = params.collab_type.value();
  let workspace_id = Uuid::from_str(&params.workspace_id)?;
  // Check if the row exists and get its workspace_id
  let existing_workspace_id: Option<Uuid> = sqlx::query_scalar!(
    "SELECT workspace_id FROM af_collab WHERE oid = $1",
    &params.object_id
  )
  .fetch_optional(tx.deref_mut())
  .await?;

  match existing_workspace_id {
    Some(existing_id) => {
      if existing_id == workspace_id {
        trace!("Update existing af_collab row");
        sqlx::query!(
          "UPDATE af_collab \
        SET blob = $2, len = $3, partition_key = $4, encrypt = $5, owner_uid = $6 WHERE oid = $1",
          params.object_id,
          params.raw_data,
          params.raw_data.len() as i32,
          partition_key,
          encrypt,
          owner_uid,
        )
        .execute(tx.deref_mut())
        .await
        .context(format!(
          "Update af_collab failed: {}:{}",
          owner_uid, params.object_id
        ))?;
      } else {
        return Err(DatabaseError::Internal(anyhow::anyhow!(
          "Inserting a row with an existing object_id but different workspace_id"
        )));
      }
    },
    None => {
      // Get the 'Owner' role_id from af_roles
      let role_id: i32 = sqlx::query_scalar!("SELECT id FROM af_roles WHERE name = 'Owner'")
        .fetch_one(tx.deref_mut())
        .await?;

      trace!(
        "Insert new af_collab row: {}:{}:{}",
        owner_uid,
        params.object_id,
        params.workspace_id
      );

      // Insert into af_collab_member
      sqlx::query!(
        "INSERT INTO af_collab_member (oid, uid, role_id) VALUES ($1, $2, $3)",
        params.object_id,
        owner_uid,
        role_id
      )
      .execute(tx.deref_mut())
      .await
      .context(format!(
        "Insert af_collab_member failed: {}:{}:{}",
        owner_uid, params.object_id, role_id
      ))?;

      sqlx::query!(
        "INSERT INTO af_collab (oid, blob, len, partition_key, encrypt, owner_uid, workspace_id)\
          VALUES ($1, $2, $3, $4, $5, $6, $7)",
        params.object_id,
        params.raw_data,
        params.raw_data.len() as i32,
        partition_key,
        encrypt,
        owner_uid,
        workspace_id,
      )
      .execute(tx.deref_mut())
      .await
      .context(format!(
        "Insert new af_collab failed: {}:{}:{}",
        owner_uid, params.object_id, params.collab_type
      ))?;
    },
  }

  Ok(())
}

pub async fn get_collab_blob(
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

pub async fn batch_get_collab_blob(
  pg_pool: &PgPool,
  queries: Vec<QueryCollabParams>,
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
