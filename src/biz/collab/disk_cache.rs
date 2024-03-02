use anyhow::anyhow;
use app_error::AppError;
use collab::core::collab_plugin::EncodedCollab;
use database::collab::{
  batch_select_collab_blob, collab_exists, create_snapshot_and_maintain_limit, delete_collab,
  get_all_collab_snapshot_meta, insert_into_af_collab, is_collab_exists,
  select_blob_from_af_collab, select_snapshot, should_create_snapshot, DatabaseResult,
  COLLAB_SNAPSHOT_LIMIT,
};
use database_entity::dto::{
  AFSnapshotMeta, AFSnapshotMetas, CollabParams, InsertSnapshotParams, QueryCollab,
  QueryCollabParams, QueryCollabResult, SnapshotData,
};
use sqlx::{PgPool, Transaction};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, event, warn, Level};
use validator::Validate;

#[derive(Clone)]
pub struct CollabDiskCache {
  pub pg_pool: PgPool,
  config: database::collab::WriteConfig,
}

impl CollabDiskCache {
  pub fn new(pg_pool: PgPool) -> Self {
    let config = database::collab::WriteConfig::default();
    Self { pg_pool, config }
  }
  pub fn config(&self) -> &database::collab::WriteConfig {
    &self.config
  }

  pub async fn is_exist(&self, object_id: &str) -> bool {
    collab_exists(&self.pg_pool, object_id)
      .await
      .unwrap_or(false)
  }

  pub async fn is_collab_exist(&self, oid: &str) -> DatabaseResult<bool> {
    let is_exist = is_collab_exists(oid, &self.pg_pool).await?;
    Ok(is_exist)
  }

  pub async fn upsert_collab_with_transaction(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> DatabaseResult<()> {
    insert_into_af_collab(transaction, uid, workspace_id, &params).await?;
    Ok(())
  }

  pub async fn get_collab_encoded(
    &self,
    _uid: &i64,
    params: QueryCollabParams,
  ) -> Result<EncodedCollab, AppError> {
    event!(
      Level::INFO,
      "Get encoded collab:{} from disk",
      params.object_id
    );

    const MAX_ATTEMPTS: usize = 3;
    let mut attempts = 0;

    loop {
      let result =
        select_blob_from_af_collab(&self.pg_pool, &params.collab_type, &params.object_id).await;

      match result {
        Ok(data) => {
          return tokio::task::spawn_blocking(move || {
            EncodedCollab::decode_from_bytes(&data).map_err(|err| {
              AppError::Internal(anyhow!("fail to decode data to EncodedCollab: {:?}", err))
            })
          })
          .await?;
        },
        Err(e) => {
          // Handle non-retryable errors immediately
          if matches!(e, sqlx::Error::RowNotFound) {
            let msg = format!("Can't find the row for query: {:?}", params);
            return Err(AppError::RecordNotFound(msg));
          }

          // Increment attempts and retry if below MAX_ATTEMPTS and the error is retryable
          if attempts < MAX_ATTEMPTS - 1 && matches!(e, sqlx::Error::PoolTimedOut) {
            attempts += 1;
            sleep(Duration::from_millis(500 * attempts as u64)).await;
            continue;
          } else {
            return Err(e.into());
          }
        },
      }
    }
  }

  pub async fn batch_get_collab(
    &self,
    _uid: &i64,
    queries: Vec<QueryCollab>,
  ) -> HashMap<String, QueryCollabResult> {
    batch_select_collab_blob(&self.pg_pool, queries).await
  }

  pub async fn delete_collab(&self, _uid: &i64, object_id: &str) -> DatabaseResult<()> {
    delete_collab(&self.pg_pool, object_id).await?;
    Ok(())
  }

  pub async fn should_create_snapshot(&self, oid: &str) -> bool {
    if oid.is_empty() {
      warn!("unexpected empty object id when checking should_create_snapshot");
      return false;
    }

    should_create_snapshot(oid, &self.pg_pool)
      .await
      .unwrap_or(false)
  }

  pub async fn create_snapshot(
    &self,
    params: InsertSnapshotParams,
  ) -> DatabaseResult<AFSnapshotMeta> {
    params.validate()?;

    debug!("create snapshot for object:{}", params.object_id);
    match self.pg_pool.try_begin().await {
      Ok(Some(transaction)) => {
        let meta = create_snapshot_and_maintain_limit(
          transaction,
          &params.workspace_id,
          &params.object_id,
          &params.encoded_collab_v1,
          COLLAB_SNAPSHOT_LIMIT,
        )
        .await?;
        Ok(meta)
      },
      _ => Err(AppError::Internal(anyhow!(
        "fail to acquire transaction to create snapshot for object:{}",
        params.object_id,
      ))),
    }
  }

  pub async fn get_collab_snapshot(&self, snapshot_id: &i64) -> DatabaseResult<SnapshotData> {
    match select_snapshot(&self.pg_pool, snapshot_id).await? {
      None => Err(AppError::RecordNotFound(format!(
        "Can't find the snapshot with id:{}",
        snapshot_id
      ))),
      Some(row) => Ok(SnapshotData {
        object_id: row.oid,
        encoded_collab_v1: row.blob,
        workspace_id: row.workspace_id.to_string(),
      }),
    }
  }

  /// Returns list of snapshots for given object_id in descending order of creation time.
  pub async fn get_collab_snapshot_list(&self, oid: &str) -> DatabaseResult<AFSnapshotMetas> {
    let metas = get_all_collab_snapshot_meta(&self.pg_pool, oid).await?;
    Ok(metas)
  }
}
