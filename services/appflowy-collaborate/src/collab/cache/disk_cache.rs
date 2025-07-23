use crate::collab::cache::encode_collab_from_bytes_with_thread_pool;
use crate::CollabMetrics;
use anyhow::{anyhow, Context};
use app_error::AppError;
use appflowy_proto::Rid;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use collab::entity::{EncodedCollab, EncoderVersion};
use collab_entity::CollabType;
use database::collab::{
  batch_select_collab_blob, insert_into_af_collab, insert_into_af_collab_bulk_for_user,
  is_collab_exists, select_blob_from_af_collab, select_collabs_created_since, AppResult,
};
use database::file::s3_client_impl::AwsS3BucketClientImpl;
use database::file::{BucketClient, ResponseBlob};
use database_entity::dto::{
  CollabParams, CollabUpdateData, PendingCollabWrite, QueryCollab, QueryCollabResult,
  ZSTD_COMPRESSION_LEVEL,
};
use infra::thread_pool::ThreadPoolNoAbort;
use sqlx::{Error, PgPool, Transaction};
use std::collections::HashMap;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::{debug, error, instrument, trace};
use uuid::Uuid;

#[derive(Clone)]
pub struct CollabDiskCache {
  thread_pool: Arc<ThreadPoolNoAbort>,
  pg_pool: PgPool,
  s3: AwsS3BucketClientImpl,
  s3_collab_threshold: usize,
  metrics: Arc<CollabMetrics>,
}

impl CollabDiskCache {
  pub fn new(
    thread_pool: Arc<ThreadPoolNoAbort>,
    pg_pool: PgPool,
    s3: AwsS3BucketClientImpl,
    s3_collab_threshold: usize,
    metrics: Arc<CollabMetrics>,
  ) -> Self {
    Self {
      thread_pool,
      pg_pool,
      s3,
      s3_collab_threshold,
      metrics,
    }
  }

  pub async fn is_exist(&self, workspace_id: &Uuid, object_id: &Uuid) -> AppResult<bool> {
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
    workspace_id: &Uuid,
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

    let start = Instant::now();
    Self::upsert_collab_with_transaction(
      workspace_id,
      uid,
      params,
      &mut transaction,
      self.s3.clone(),
      self.s3_collab_threshold,
      &self.metrics,
    )
    .await?;

    tokio::time::timeout(Duration::from_secs(10), transaction.commit())
      .await
      .map_err(|_| {
        AppError::Internal(anyhow!(
          "Timeout when committing the transaction for pending collaboration data"
        ))
      })??;
    self.metrics.observe_pg_tx(start.elapsed());

    Ok(())
  }

  pub fn s3_client(&self) -> AwsS3BucketClientImpl {
    self.s3.clone()
  }

  pub async fn upsert_collab_with_transaction(
    workspace_id: &Uuid,
    uid: &i64,
    mut params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    s3: AwsS3BucketClientImpl,
    s3_collab_threshold: usize,
    metrics: &CollabMetrics,
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
      metrics.s3_write_collab_count.inc();
    } else {
      // put collab into Postgres (and remove outdated version from S3)
      metrics.pg_write_collab_count.inc();
      delete_from_s3.push(key);
    }

    insert_into_af_collab(transaction, uid, workspace_id, &params).await?;
    Ok(())
  }

  #[instrument(level = "trace", skip_all)]
  pub async fn get_collabs_created_since(
    &self,
    workspace_id: Uuid,
    since: DateTime<Utc>,
    limit: usize,
  ) -> Result<Vec<CollabUpdateData>, AppError> {
    let mut collabs: HashMap<_, _> =
      select_collabs_created_since(&self.pg_pool, &workspace_id, since, limit)
        .await?
        .into_iter()
        .flat_map(|record| {
          let encoded_collab = if record.blob.is_empty() {
            EncodedCollab {
              state_vector: Default::default(),
              doc_state: Default::default(),
              version: Default::default(),
            }
          } else {
            EncodedCollab::decode_from_bytes(&record.blob).ok()?
          };
          Some((
            record.oid,
            CollabUpdateData {
              object_id: record.oid,
              collab_type: CollabType::from(record.partition_key),
              encoded_collab,
              updated_at: Some(record.updated_at),
            },
          ))
        })
        .collect();
    tracing::debug!(
      "Found {} collabs created in workspace {} since {}",
      collabs.len(),
      workspace_id,
      since
    );
    let mut join_set = JoinSet::new();
    for (&oid, collab) in collabs.iter() {
      if collab.encoded_collab.doc_state.is_empty() {
        let s3 = self.s3.clone();
        self.metrics.s3_read_collab_count.inc();
        join_set.spawn(async move {
          let key = collab_key(&workspace_id, &oid);
          (oid, Self::get_collab_from_s3(&s3, key).await)
        });
      }
    }
    while let Some(Ok((oid, res))) = join_set.join_next().await {
      match res {
        Ok((rid, encoded_collab)) => {
          if let Some(collab) = collabs.get_mut(&oid) {
            // Double-check that the record hasn't been deleted since the initial query
            match self.is_collab_deleted(&oid).await {
              Ok(true) => {
                // Record was deleted, remove it from results
                tracing::warn!(
                  "Collab {} was deleted after initial query, removing from results",
                  oid
                );
                collabs.remove(&oid);
              },
              Ok(false) => {
                // Record is still valid, update with S3 data
                collab.updated_at = DateTime::from_timestamp_millis(rid.timestamp as i64);
                collab.encoded_collab = encoded_collab;
              },
              Err(err) => {
                // Error checking deletion status, remove from results to be safe
                tracing::warn!(
                  "Error checking deletion status for collab {}: {}, removing from results",
                  oid,
                  err
                );
                collabs.remove(&oid);
              },
            }
          }
        },
        Err(err) => {
          tracing::warn!("failed to get collab {} state from S3: {}", oid, err);
          collabs.remove(&oid);
        },
      }
    }
    Ok(collabs.into_values().collect())
  }

  async fn get_collab_from_s3(
    s3: &AwsS3BucketClientImpl,
    key: String,
  ) -> Result<(Rid, EncodedCollab), AppError> {
    match s3.get_blob(&key).await {
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
        let encoded_collab = EncodedCollab {
          state_vector: Default::default(),
          doc_state: decompressed.into(),
          version: EncoderVersion::V1,
        };
        let rid = Rid::default(); //TODO: we need to store it somewhere
        Ok((rid, encoded_collab))
      },
      Err(err) => Err(err),
    }
  }

  #[instrument(level = "trace", skip_all)]
  pub async fn get_encoded_collab_from_disk(
    &self,
    workspace_id: &Uuid,
    query: QueryCollab,
  ) -> Result<(Rid, EncodedCollab), AppError> {
    debug!("try get {}:{} from s3", query.collab_type, query.object_id);
    let key = collab_key(workspace_id, &query.object_id);

    let is_deleted = self.is_collab_deleted(&query.object_id).await?;
    if is_deleted {
      return Err(AppError::RecordDeleted(format!(
        "{}/{} is deleted from db",
        query.collab_type, query.object_id
      )));
    }

    match Self::get_collab_from_s3(&self.s3, key).await {
      Ok((rid, encoded_collab)) => {
        self.metrics.s3_read_collab_count.inc();
        return Ok((rid, encoded_collab));
      },
      Err(AppError::RecordNotFound(_)) => {
        debug!(
          "Can not find the {}/{} from s3, trying to get from Postgres",
          query.collab_type, query.object_id
        );
      },
      Err(e) => return Err(e),
    }

    const MAX_ATTEMPTS: usize = 3;
    let mut attempts = 0;

    loop {
      let result =
        select_blob_from_af_collab(&self.pg_pool, &query.collab_type, &query.object_id).await;

      match result {
        Ok((updated_at, data)) => {
          self.metrics.pg_read_collab_count.inc();
          let rid = Rid::new(updated_at.timestamp_millis() as u64, 0);
          let encoded_collab =
            encode_collab_from_bytes_with_thread_pool(&self.thread_pool, data).await?;
          return Ok((rid, encoded_collab));
        },
        Err(e) => {
          match e {
            Error::RowNotFound => {
              debug!(
                "Can not find the {}/{} from Postgres",
                query.object_id, query.collab_type
              );
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
    workspace_id: Uuid,
    uid: &i64,
    mut params_list: Vec<CollabParams>,
  ) -> Result<(), AppError> {
    if params_list.is_empty() {
      return Ok(());
    }

    let mut delete_from_s3 = Vec::new();
    let mut blobs = HashMap::new();
    for param in params_list.iter_mut() {
      let key = collab_key(&workspace_id, &param.object_id);
      if param.encoded_collab_v1.len() > self.s3_collab_threshold {
        let blob = std::mem::take(&mut param.encoded_collab_v1);
        blobs.insert(key, blob);
      } else {
        // put collab into Postgres (and remove outdated version from S3)
        delete_from_s3.push(key);
      }
    }
    let s3_count = blobs.len() as u64;
    let pg_count = delete_from_s3.len() as u64;

    let mut transaction = self.pg_pool.begin().await?;
    let start = Instant::now();
    insert_into_af_collab_bulk_for_user(&mut transaction, uid, workspace_id, &params_list).await?;
    transaction.commit().await?;
    self.metrics.observe_pg_tx(start.elapsed());

    batch_put_collab_to_s3(&self.s3, blobs).await?;
    if !delete_from_s3.is_empty() {
      let s3 = self.s3.clone();
      tokio::spawn(async move {
        if let Err(err) = s3.delete_blobs(delete_from_s3).await {
          tracing::warn!("failed to delete outdated collabs from S3: {}", err);
        }
      });
    }
    self.metrics.s3_write_collab_count.inc_by(s3_count);
    self.metrics.pg_write_collab_count.inc_by(pg_count);
    Ok(())
  }

  pub async fn batch_insert_collab(
    &self,
    records: Vec<PendingCollabWrite>,
  ) -> Result<(), AppError> {
    if records.is_empty() {
      return Ok(());
    }

    let s3 = self.s3.clone();
    // Start a database transaction
    let mut transaction = self
      .pg_pool
      .begin()
      .await
      .context("Failed to acquire transaction for writing pending collaboration data")
      .map_err(AppError::from)?;
    let start = Instant::now();

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
        &self.metrics,
      )
      .await
      {
        sqlx::query(&format!("ROLLBACK TO SAVEPOINT {}", savepoint_name))
          .execute(transaction.deref_mut())
          .await?;
      }
    }

    // Commit the transaction to finalize all writes
    match tokio::time::timeout(Duration::from_secs(10), transaction.commit()).await {
      Ok(result) => {
        self.metrics.observe_pg_tx(start.elapsed());
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
    Ok(())
  }

  /// Batch retrieves collaboration data for multiple queries.
  ///
  /// This method first checks if any of the requested collaborations have been deleted
  /// and filters them out before attempting to retrieve data from S3 or PostgreSQL.
  /// This is important because when a collaboration is deleted, it's only marked as
  /// deleted in PostgreSQL (deleted_at field), but the S3 blob might still exist.
  pub async fn batch_get_collab(
    &self,
    workspace_id: &Uuid,
    queries: Vec<QueryCollab>,
  ) -> HashMap<Uuid, QueryCollabResult> {
    // Filter out deleted collabs before processing
    let mut valid_queries = Vec::new();
    let mut results = HashMap::new();

    // Batch check deletion status for all queries
    let object_ids: Vec<Uuid> = queries.iter().map(|q| q.object_id).collect();
    let deletion_status = match self.batch_is_collab_deleted(&object_ids).await {
      Ok(status) => status,
      Err(err) => {
        // If batch check fails, mark all queries as failed
        for query in queries {
          results.insert(
            query.object_id,
            QueryCollabResult::Failed {
              error: format!("Error checking deletion status: {}", err),
            },
          );
        }
        return results;
      },
    };

    for query in queries {
      let is_deleted = deletion_status.get(&query.object_id).unwrap_or(&false);
      if *is_deleted {
        // Record is deleted, add error result
        results.insert(
          query.object_id,
          QueryCollabResult::Failed {
            error: format!(
              "Collaboration record for {}:{} is deleted",
              query.collab_type, query.object_id
            ),
          },
        );
      } else {
        // Record is not deleted, add to valid queries
        valid_queries.push(query);
      }
    }

    let not_found =
      batch_get_collab_from_s3(&self.s3, workspace_id, valid_queries, &mut results).await;
    let s3_fetch = results.len() as u64;
    batch_select_collab_blob(&self.pg_pool, not_found, &mut results).await;
    let pg_fetch = results.len() as u64 - s3_fetch;
    self.metrics.s3_read_collab_count.inc_by(s3_fetch);
    self.metrics.pg_read_collab_count.inc_by(pg_fetch);
    results
  }

  pub async fn delete_collab(&self, workspace_id: &Uuid, object_id: &Uuid) -> AppResult<()> {
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

    trace!("record {}:{} marked as deleted", workspace_id, object_id);
    let key = collab_key(workspace_id, object_id);
    match self.s3.delete_blob(&key).await {
      Ok(_) | Err(AppError::RecordNotFound(_)) => Ok(()),
      Err(err) => Err(err),
    }
  }

  pub async fn is_collab_deleted(&self, object_id: &Uuid) -> AppResult<bool> {
    let result = sqlx::query!(
      r#"
        SELECT deleted_at IS NOT NULL AS is_deleted
        FROM af_collab
        WHERE oid = $1;
      "#,
      object_id
    )
    .fetch_one(&self.pg_pool)
    .await?;

    Ok(result.is_deleted.unwrap_or(false))
  }

  pub async fn batch_is_collab_deleted(
    &self,
    object_ids: &[Uuid],
  ) -> AppResult<HashMap<Uuid, bool>> {
    if object_ids.is_empty() {
      return Ok(HashMap::new());
    }

    let results = sqlx::query!(
      r#"
        SELECT oid, deleted_at IS NOT NULL AS is_deleted
        FROM af_collab
        WHERE oid = ANY($1);
      "#,
      object_ids
    )
    .fetch_all(&self.pg_pool)
    .await?;

    let mut deletion_status = HashMap::new();
    for result in results {
      deletion_status.insert(result.oid, result.is_deleted.unwrap_or(false));
    }

    // For object_ids not found in the database, consider them as not deleted (they might not exist yet)
    for &object_id in object_ids {
      deletion_status.entry(object_id).or_insert(false);
    }

    Ok(deletion_status)
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
          tracing::debug!(
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
    tracing::trace!("saved collab to S3: {}", key);
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
  workspace_id: &Uuid,
  params: Vec<QueryCollab>,
  results: &mut HashMap<Uuid, QueryCollabResult>,
) -> Vec<QueryCollab> {
  enum GetResult {
    Found(Uuid, Vec<u8>),
    NotFound(QueryCollab),
    Error(Uuid, String),
  }

  async fn gather(
    join_set: &mut JoinSet<GetResult>,
    results: &mut HashMap<Uuid, QueryCollabResult>,
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

fn collab_key_prefix(workspace_id: &Uuid, object_id: &Uuid) -> String {
  format!("collabs/{}/{}/", workspace_id, object_id)
}

fn collab_key(workspace_id: &Uuid, object_id: &Uuid) -> String {
  format!(
    "collabs/{}/{}/encoded_collab.v1.zstd",
    workspace_id, object_id
  )
}
