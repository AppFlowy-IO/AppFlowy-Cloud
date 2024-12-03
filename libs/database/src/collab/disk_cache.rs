use anyhow::{anyhow, Context};
use bytes::Bytes;
use collab::entity::{EncodedCollab, EncoderVersion};
use collab_entity::CollabType;
use sqlx::{Error, PgPool, Transaction};
use std::collections::HashMap;
use std::ops::DerefMut;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::{error, instrument};
use uuid::Uuid;

use crate::collab::util::encode_collab_from_bytes;
use crate::collab::{
  batch_select_collab_blob, insert_into_af_collab, insert_into_af_collab_bulk_for_user,
  is_collab_exists, select_blob_from_af_collab, AppResult,
};
use crate::file::s3_client_impl::AwsS3BucketClientImpl;
use crate::file::{BucketClient, ResponseBlob};
use crate::index::upsert_collab_embeddings;
use app_error::AppError;
use database_entity::dto::{
  CollabParams, PendingCollabWrite, QueryCollab, QueryCollabResult, ZSTD_COMPRESSION_LEVEL,
};

#[derive(Clone)]
pub struct CollabDiskCache {
  pg_pool: PgPool,
  s3: AwsS3BucketClientImpl,
  s3_collab_threshold: usize,
}

impl CollabDiskCache {
  pub fn new(pg_pool: PgPool, s3: AwsS3BucketClientImpl, s3_collab_threshold: usize) -> Self {
    Self {
      pg_pool,
      s3,
      s3_collab_threshold,
    }
  }

  pub async fn is_exist(&self, workspace_id: &str, object_id: &str) -> AppResult<bool> {
    let dir = collab_key_prefix(workspace_id, object_id);
    let resp = self.s3.list_dir(&dir, 1).await?;
    if resp.is_empty() {
      // fallback to Postgres
      Ok(is_collab_exists(object_id, &self.pg_pool).await?)
    } else {
      Ok(true)
    }
  }

  pub async fn upsert_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
  ) -> AppResult<()> {
    // Start a database transaction
    let mut transaction = self
      .pg_pool
      .begin()
      .await
      .context("Failed to acquire transaction for writing pending collaboration data")
      .map_err(AppError::from)?;

    Self::upsert_collab_with_transaction(
      workspace_id,
      uid,
      params,
      &mut transaction,
      self.s3.clone(),
      self.s3_collab_threshold,
    )
    .await?;

    tokio::time::timeout(Duration::from_secs(10), transaction.commit())
      .await
      .map_err(|_| {
        AppError::Internal(anyhow!(
          "Timeout when committing the transaction for pending collaboration data"
        ))
      })??;

    Ok(())
  }

  pub fn s3_client(&self) -> AwsS3BucketClientImpl {
    self.s3.clone()
  }

  pub async fn upsert_collab_with_transaction(
    workspace_id: &str,
    uid: &i64,
    mut params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    s3: AwsS3BucketClientImpl,
    s3_collab_threshold: usize,
  ) -> AppResult<()> {
    let mut delete_from_s3 = Vec::new();
    let key = collab_key(workspace_id, &params.object_id);
    if params.encoded_collab_v1.len() > s3_collab_threshold {
      // put collab into S3
      let encoded_collab = std::mem::take(&mut params.encoded_collab_v1);
      tokio::spawn(Self::insert_blob_with_retries(
        s3.clone(),
        key,
        encoded_collab,
        3,
      ));
    } else {
      // put collab into Postgres (and remove outdated version from S3)
      delete_from_s3.push(key);
    }

    insert_into_af_collab(transaction, uid, workspace_id, &params).await?;
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
      if !delete_from_s3.is_empty() {
        tokio::spawn(async move {
          if let Err(err) = s3.delete_blobs(delete_from_s3).await {
            tracing::warn!("failed to delete outdated collab from S3: {}", err);
          }
        });
      }
    } else if params.collab_type == CollabType::Document {
      tracing::info!("no embeddings to save for collab {}", params.object_id);
    }

    Ok(())
  }

  #[instrument(level = "trace", skip_all)]
  pub async fn get_collab_encoded_from_disk(
    &self,
    workspace_id: &str,
    query: QueryCollab,
  ) -> Result<EncodedCollab, AppError> {
    tracing::debug!("try get {}:{} from s3", query.collab_type, query.object_id);
    let key = collab_key(workspace_id, &query.object_id);
    match self.s3.get_blob(&key).await {
      Ok(resp) => {
        let blob = resp.to_blob();
        let now = Instant::now();
        let decompressed = zstd::decode_all(&*blob)?;
        tracing::trace!(
          "decompressed collab {}B -> {}B in {:?}",
          blob.len(),
          decompressed.len(),
          now.elapsed()
        );
        return Ok(EncodedCollab {
          state_vector: Default::default(),
          doc_state: decompressed.into(),
          version: EncoderVersion::V1,
        });
      },
      Err(AppError::RecordNotFound(_)) => {
        tracing::debug!(
          "try get {}:{} from database",
          query.collab_type,
          query.object_id
        );
      },
      Err(err) => {
        return Err(err);
      },
    }

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
    mut params_list: Vec<CollabParams>,
  ) -> Result<(), AppError> {
    if params_list.is_empty() {
      return Ok(());
    }

    let mut delete_from_s3 = Vec::new();
    let mut blobs = HashMap::new();
    for param in params_list.iter_mut() {
      let key = collab_key(workspace_id, &param.object_id);
      if param.encoded_collab_v1.len() > self.s3_collab_threshold {
        let blob = std::mem::take(&mut param.encoded_collab_v1);
        blobs.insert(key, blob);
      } else {
        // put collab into Postgres (and remove outdated version from S3)
        delete_from_s3.push(key);
      }
    }

    let mut transaction = self.pg_pool.begin().await?;
    insert_into_af_collab_bulk_for_user(&mut transaction, uid, workspace_id, &params_list).await?;
    transaction.commit().await?;

    batch_put_collab_to_s3(&self.s3, blobs).await?;
    if !delete_from_s3.is_empty() {
      self.s3.delete_blobs(delete_from_s3).await?;
    }
    Ok(())
  }

  pub async fn batch_insert_collab(
    &self,
    records: Vec<PendingCollabWrite>,
  ) -> Result<u64, AppError> {
    if records.is_empty() {
      return Ok(0);
    }

    let s3 = self.s3.clone();
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
        params,
        &mut transaction,
        s3.clone(),
        self.s3_collab_threshold,
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
      },
      Err(_) => {
        error!(
          "Timeout waiting for committing the transaction for pending write:{}",
          action_description
        );
        return Err(AppError::Internal(anyhow!(
          "Timeout when committing the transaction for pending collaboration data"
        )));
      },
    }
    Ok(successful_writes)
  }

  pub async fn batch_get_collab(
    &self,
    workspace_id: &str,
    queries: Vec<QueryCollab>,
  ) -> HashMap<String, QueryCollabResult> {
    let mut results = HashMap::new();
    let not_found = batch_get_collab_from_s3(&self.s3, workspace_id, queries, &mut results).await;
    batch_select_collab_blob(&self.pg_pool, not_found, &mut results).await;
    results
  }

  pub async fn delete_collab(&self, workspace_id: &str, object_id: &str) -> AppResult<()> {
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
    let key = collab_key(workspace_id, object_id);
    match self.s3.delete_blob(&key).await {
      Ok(_) | Err(AppError::RecordNotFound(_)) => Ok(()),
      Err(err) => Err(err),
    }
  }

  async fn insert_blob_with_retries(
    s3: AwsS3BucketClientImpl,
    key: String,
    blob: Bytes,
    mut retries: usize,
  ) -> Result<(), AppError> {
    let doc_state = Self::compress_encoded_collab(blob)?;
    while let Err(err) = s3.put_blob(&key, doc_state.clone().into(), None).await {
      match err {
        AppError::ServiceTemporaryUnavailable(err) if retries > 0 => {
          tracing::info!(
            "S3 service is temporarily unavailable: {}. Remaining retries: {}",
            err,
            retries
          );
          retries -= 1;
          sleep(Duration::from_secs(5)).await;
        },
        err => {
          tracing::error!("Failed to save collab to S3: {}", err);
          break;
        },
      }
    }
    Ok(())
  }

  fn compress_encoded_collab(encoded_collab_v1: Bytes) -> Result<Bytes, AppError> {
    let encoded_collab = EncodedCollab::decode_from_bytes(&encoded_collab_v1)
      .map_err(|err| AppError::Internal(err.into()))?;
    let now = Instant::now();
    let doc_state = zstd::encode_all(&*encoded_collab.doc_state, ZSTD_COMPRESSION_LEVEL)?;
    tracing::trace!(
      "compressed collab {}B -> {}B in {:?}",
      encoded_collab_v1.len(),
      doc_state.len(),
      now.elapsed()
    );
    Ok(doc_state.into())
  }
}

async fn batch_put_collab_to_s3(
  s3: &AwsS3BucketClientImpl,
  collabs: HashMap<String, Bytes>,
) -> Result<(), AppError> {
  let mut join_set = JoinSet::<Result<(), AppError>>::new();
  let mut i = 0;
  for (key, blob) in collabs {
    let s3 = s3.clone();
    join_set.spawn(async move {
      let compressed = CollabDiskCache::compress_encoded_collab(blob)?;
      s3.put_blob(&key, compressed.into(), None).await?;
      Ok(())
    });
    i += 1;
    if i % 500 == 0 {
      while let Some(result) = join_set.join_next().await {
        result.map_err(|err| AppError::Internal(err.into()))??;
      }
    }
  }

  while let Some(result) = join_set.join_next().await {
    result.map_err(|err| AppError::Internal(err.into()))??;
  }

  Ok(())
}

async fn batch_get_collab_from_s3(
  s3: &AwsS3BucketClientImpl,
  workspace_id: &str,
  params: Vec<QueryCollab>,
  results: &mut HashMap<String, QueryCollabResult>,
) -> Vec<QueryCollab> {
  enum GetResult {
    Found(String, Vec<u8>),
    NotFound(QueryCollab),
    Error(String, String),
  }

  async fn gather(
    join_set: &mut JoinSet<GetResult>,
    results: &mut HashMap<String, QueryCollabResult>,
    not_found: &mut Vec<QueryCollab>,
  ) {
    while let Some(result) = join_set.join_next().await {
      let now = Instant::now();
      match result {
        Ok(GetResult::Found(object_id, compressed)) => match zstd::decode_all(&*compressed) {
          Ok(decompressed) => {
            tracing::trace!(
              "decompressed collab {}B -> {}B in {:?}",
              compressed.len(),
              decompressed.len(),
              now.elapsed()
            );
            let encoded_collab = EncodedCollab {
              state_vector: Default::default(),
              doc_state: decompressed.into(),
              version: EncoderVersion::V1,
            };
            results.insert(
              object_id,
              QueryCollabResult::Success {
                encode_collab_v1: encoded_collab.encode_to_bytes().unwrap(),
              },
            );
          },
          Err(err) => {
            results.insert(
              object_id,
              QueryCollabResult::Failed {
                error: err.to_string(),
              },
            );
          },
        },
        Ok(GetResult::NotFound(query)) => not_found.push(query),
        Ok(GetResult::Error(object_id, error)) => {
          results.insert(object_id, QueryCollabResult::Failed { error });
        },
        Err(err) => error!("Failed to get collab from S3: {}", err),
      }
    }
  }

  let mut not_found = Vec::new();
  let mut i = 0;
  let mut join_set = JoinSet::new();
  for query in params {
    let key = collab_key(workspace_id, &query.object_id);
    let s3 = s3.clone();
    join_set.spawn(async move {
      match s3.get_blob(&key).await {
        Ok(resp) => GetResult::Found(query.object_id, resp.to_blob()),
        Err(AppError::RecordNotFound(_)) => GetResult::NotFound(query),
        Err(err) => GetResult::Error(query.object_id, err.to_string()),
      }
    });
    i += 1;
    if i % 500 == 0 {
      gather(&mut join_set, results, &mut not_found).await;
    }
  }
  // gather remaining results from the last chunk
  gather(&mut join_set, results, &mut not_found).await;
  not_found
}

fn collab_key_prefix(workspace_id: &str, object_id: &str) -> String {
  format!("collabs/{}/{}/", workspace_id, object_id)
}

fn collab_key(workspace_id: &str, object_id: &str) -> String {
  format!(
    "collabs/{}/{}/encoded_collab.v1.zstd",
    workspace_id, object_id
  )
}
