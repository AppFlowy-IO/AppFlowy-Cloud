use anyhow::{anyhow, Context};
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use sqlx::{Error, PgPool, Transaction};
use std::collections::HashMap;
use std::ops::DerefMut;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, event, instrument, Level};
use uuid::Uuid;

use crate::collab::util::encode_collab_from_bytes;
use crate::collab::{
  batch_select_collab_blob, insert_into_af_collab, insert_into_af_collab_bulk_for_user,
  is_collab_exists, select_blob_from_af_collab, select_collab_meta_from_af_collab, AppResult,
};
use crate::file::s3_client_impl::AwsS3BucketClientImpl;
use crate::file::BucketClient;
use crate::index::upsert_collab_embeddings;
use crate::pg_row::AFCollabRowMeta;
use app_error::AppError;
use database_entity::dto::{CollabParams, PendingCollabWrite, QueryCollab, QueryCollabResult};

#[derive(Clone)]
pub struct CollabDiskCache {
  pg_pool: PgPool,
  s3: AwsS3BucketClientImpl,
}

impl CollabDiskCache {
  pub fn new(pg_pool: PgPool, s3: AwsS3BucketClientImpl) -> Self {
    Self { pg_pool, s3 }
  }

  pub async fn is_exist(&self, workspace_id: &str, object_id: &str) -> AppResult<bool> {
    let is_exist = is_collab_exists(object_id, &self.pg_pool).await?;
    Ok(is_exist)
  }

  pub async fn upsert_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: &CollabParams,
  ) -> AppResult<()> {
    // Start a database transaction
    let mut transaction = self
      .pg_pool
      .begin()
      .await
      .context("Failed to acquire transaction for writing pending collaboration data")
      .map_err(AppError::from)?;

    Self::upsert_collab_with_transaction(workspace_id, uid, params, &mut transaction).await?;

    tokio::time::timeout(Duration::from_secs(10), transaction.commit())
      .await
      .map_err(|_| {
        AppError::Internal(anyhow!(
          "Timeout when committing the transaction for pending collaboration data"
        ))
      })??;

    Ok(())
  }

  pub async fn upsert_collab_with_transaction(
    workspace_id: &str,
    uid: &i64,
    params: &CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> AppResult<()> {
    insert_into_af_collab(transaction, uid, workspace_id, params).await?;
    if let Some(em) = &params.embeddings {
      tracing::info!(
        "saving collab {} embeddings (cost: {} tokens)",
        params.object_id,
        em.tokens_consumed
      );
      let workspace_id = Uuid::parse_str(workspace_id)?;
      upsert_collab_embeddings(
        transaction,
        &workspace_id,
        em.tokens_consumed,
        em.params.clone(),
      )
      .await?;
    } else if params.collab_type == CollabType::Document {
      tracing::info!("no embeddings to save for collab {}", params.object_id);
    }

    Ok(())
  }

  #[instrument(level = "trace", skip_all)]
  pub async fn get_collab_encoded_from_disk(
    &self,
    query: QueryCollab,
  ) -> Result<EncodedCollab, AppError> {
    event!(
      Level::DEBUG,
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
          return encode_collab_from_bytes(data).await;
        },
        Err(e) => {
          match e {
            Error::RowNotFound => {
              let msg = format!("Can't find the row for query: {:?}", query);
              return Err(AppError::RecordNotFound(msg));
            },
            _ => {
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
        },
      }
    }
  }

  //FIXME: this and `batch_insert_collab` duplicate similar logic.
  pub async fn bulk_insert_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params_list: Vec<CollabParams>,
  ) -> Result<(), AppError> {
    let mut transaction = self.pg_pool.begin().await?;
    insert_into_af_collab_bulk_for_user(&mut transaction, uid, workspace_id, &params_list).await?;
    transaction.commit().await?;
    Ok(())
  }

  pub async fn batch_insert_collab(
    &self,
    records: Vec<PendingCollabWrite>,
  ) -> Result<u64, AppError> {
    // Start a database transaction
    let mut transaction = self
      .pg_pool
      .begin()
      .await
      .context("Failed to acquire transaction for writing pending collaboration data")
      .map_err(AppError::from)?;

    let mut successful_writes = 0;
    // Insert each record into the database within the transaction context
    let mut action_description = String::new();
    for (index, record) in records.into_iter().enumerate() {
      let params = record.params;
      action_description = format!("{}", params);
      let savepoint_name = format!("sp_{}", index);

      // using savepoint to rollback the transaction if the insert fails
      sqlx::query(&format!("SAVEPOINT {}", savepoint_name))
        .execute(transaction.deref_mut())
        .await?;
      if let Err(_err) = Self::upsert_collab_with_transaction(
        &record.workspace_id,
        &record.uid,
        &params,
        &mut transaction,
      )
      .await
      {
        sqlx::query(&format!("ROLLBACK TO SAVEPOINT {}", savepoint_name))
          .execute(transaction.deref_mut())
          .await?;
      } else {
        successful_writes += 1;
      }
    }

    // Commit the transaction to finalize all writes
    match tokio::time::timeout(Duration::from_secs(10), transaction.commit()).await {
      Ok(result) => {
        result.map_err(AppError::from)?;
        Ok(successful_writes)
      },
      Err(_) => {
        error!(
          "Timeout waiting for committing the transaction for pending write:{}",
          action_description
        );
        Err(AppError::Internal(anyhow!(
          "Timeout when committing the transaction for pending collaboration data"
        )))
      },
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

fn collab_key(workspace_id: &str, object_id: &str) -> String {
  format!("collabs/{}/{}/collab", workspace_id, object_id)
}
