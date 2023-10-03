use anyhow::Context;
use collab_define::CollabType;
use database_entity::error::DatabaseError;
use std::ops::DerefMut;

use sqlx::types::Uuid;
use sqlx::PgPool;
use tracing::trace;

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
  tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
  workspace_id: &Uuid,
  owner_uid: i64,
  object_id: &str,
  raw_data: &[u8],
  collab_type: &CollabType,
) -> Result<(), DatabaseError> {
  let encrypt = 0;

  let partition_key = collab_type.value();

  // Check if the row exists and get its workspace_id
  let existing_workspace_id: Option<Uuid> = sqlx::query_scalar!(
    "SELECT workspace_id FROM af_collab WHERE oid = $1",
    object_id,
  )
  .fetch_optional(tx.deref_mut())
  .await?;

  match existing_workspace_id {
    Some(existing_id) => {
      if &existing_id == workspace_id {
        trace!("Update existing af_collab row");
        sqlx::query!(
          r#"
            UPDATE af_collab
            SET blob = $1, len = $2, partition_key = $3, encrypt = $4, owner_uid = $5
            WHERE oid = $6
        "#,
          raw_data,
          raw_data.len() as i32,
          partition_key,
          encrypt,
          owner_uid,
          object_id,
        )
        .execute(tx.deref_mut())
        .await
        .context(format!(
          "Update af_collab failed: {}:{}",
          owner_uid, object_id
        ))?;
      } else {
        return Err(DatabaseError::from(anyhow::anyhow!(
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
        object_id,
        workspace_id,
      );

      // Insert into af_collab_member
      sqlx::query!(
        "INSERT INTO af_collab_member (oid, uid, role_id) VALUES ($1, $2, $3)",
        object_id,
        owner_uid,
        role_id
      )
      .execute(tx.deref_mut())
      .await
      .context(format!(
        "Insert af_collab_member failed: {}:{}:{}",
        owner_uid, object_id, role_id,
      ))?;

      sqlx::query!(
        "INSERT INTO af_collab (oid, blob, len, partition_key, encrypt, owner_uid, workspace_id)\
          VALUES ($1, $2, $3, $4, $5, $6, $7)",
        object_id,
        raw_data,
        raw_data.len() as i32,
        partition_key,
        encrypt,
        owner_uid,
        workspace_id,
      )
      .execute(tx.deref_mut())
      .await
      .context(format!(
        "Insert new af_collab failed: {}:{}:{}",
        owner_uid, object_id, collab_type,
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
