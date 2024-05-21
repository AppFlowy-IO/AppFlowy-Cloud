use std::collections::HashMap;
use std::time::Duration;

use anyhow::anyhow;
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use sqlx::{PgPool, Transaction};
use tokio::time::sleep;
use tracing::{event, instrument, Level};

use app_error::AppError;
use database::collab::{
  batch_select_collab_blob, insert_into_af_collab, is_collab_exists, select_blob_from_af_collab,
  select_collab_meta_from_af_collab, AppResult,
};
use database::pg_row::AFCollabRowMeta;
use database_entity::dto::{CollabParams, QueryCollab, QueryCollabResult};

#[derive(Clone)]
pub struct CollabDiskCache {
  pub pg_pool: PgPool,
}

impl CollabDiskCache {
  pub fn new(pg_pool: PgPool) -> Self {
    Self { pg_pool }
  }

  pub async fn is_exist(&self, object_id: &str) -> AppResult<bool> {
    let is_exist = is_collab_exists(object_id, &self.pg_pool).await?;
    Ok(is_exist)
  }

  pub async fn get_collab_meta(
    &self,
    object_id: &str,
    collab_type: &CollabType,
  ) -> AppResult<AFCollabRowMeta> {
    let result = select_collab_meta_from_af_collab(&self.pg_pool, object_id, collab_type).await?;
    match result {
      None => {
        let msg = format!("Can't find the row for object_id: {}", object_id);
        Err(AppError::RecordNotFound(msg))
      },
      Some(meta) => Ok(meta),
    }
  }

  pub async fn upsert_collab_with_transaction(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: &CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> AppResult<()> {
    insert_into_af_collab(transaction, uid, workspace_id, params).await?;
    Ok(())
  }

  #[instrument(level = "trace", skip_all)]
  pub async fn get_collab_encoded_from_disk(
    &self,
    _uid: &i64,
    query: QueryCollab,
  ) -> Result<EncodedCollab, AppError> {
    event!(
      Level::INFO,
      "try get {}:{} from disk",
      query.collab_type,
      query.object_id
    );

    const MAX_ATTEMPTS: usize = 3;
    let mut attempts = 0;

    loop {
      let result =
        select_blob_from_af_collab(&self.pg_pool, &query.collab_type, &query.object_id).await;

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
            let msg = format!("Can't find the row for query: {:?}", query);
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
    queries: Vec<QueryCollab>,
  ) -> HashMap<String, QueryCollabResult> {
    batch_select_collab_blob(&self.pg_pool, queries).await
  }

  pub async fn delete_collab(&self, object_id: &str) -> AppResult<()> {
    sqlx::query!(
      r#"
        UPDATE af_collab
        SET deleted_at = $2
        WHERE oid = $1;
        "#,
      object_id,
      chrono::Utc::now()
    )
    .execute(&self.pg_pool)
    .await?;
    Ok(())
  }
}
