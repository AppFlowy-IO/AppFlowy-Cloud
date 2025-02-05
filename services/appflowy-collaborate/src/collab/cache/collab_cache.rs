use bytes::Bytes;
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use futures_util::{stream, StreamExt};
use itertools::{Either, Itertools};
use sqlx::{PgPool, Transaction};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, event, Level};

use super::disk_cache::CollabDiskCache;
use super::mem_cache::{cache_exp_secs_from_collab_type, CollabMemCache};
use crate::CollabMetrics;
use app_error::AppError;
use database::file::s3_client_impl::AwsS3BucketClientImpl;
use database_entity::dto::{CollabParams, PendingCollabWrite, QueryCollab, QueryCollabResult};

#[derive(Clone)]
pub struct CollabCache {
  disk_cache: CollabDiskCache,
  mem_cache: CollabMemCache,
  s3_collab_threshold: usize,
  metrics: Arc<CollabMetrics>,
}

impl CollabCache {
  pub fn new(
    redis_conn_manager: redis::aio::ConnectionManager,
    pg_pool: PgPool,
    s3: AwsS3BucketClientImpl,
    metrics: Arc<CollabMetrics>,
    s3_collab_threshold: usize,
  ) -> Self {
    let mem_cache = CollabMemCache::new(redis_conn_manager.clone(), metrics.clone());
    let disk_cache =
      CollabDiskCache::new(pg_pool.clone(), s3, s3_collab_threshold, metrics.clone());
    Self {
      disk_cache,
      mem_cache,
      s3_collab_threshold,
      metrics,
    }
  }

  pub fn metrics(&self) -> &CollabMetrics {
    &self.metrics
  }

  pub async fn bulk_insert_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params_list: Vec<CollabParams>,
  ) -> Result<(), AppError> {
    self
      .disk_cache
      .bulk_insert_collab(workspace_id, uid, params_list.clone())
      .await?;

    // update the mem cache without blocking the current task
    let mem_cache = self.mem_cache.clone();
    tokio::spawn(async move {
      let timestamp = chrono::Utc::now().timestamp();
      for params in params_list {
        if let Err(err) = mem_cache
          .insert_encode_collab_data(
            &params.object_id,
            &params.encoded_collab_v1,
            timestamp,
            Some(cache_exp_secs_from_collab_type(&params.collab_type)),
          )
          .await
          .map_err(|err| AppError::Internal(err.into()))
        {
          tracing::warn!(
            "Failed to insert collab `{}` into memory cache: {}",
            params.object_id,
            err
          );
        }
      }
    });

    Ok(())
  }

  pub async fn get_encode_collab(
    &self,
    workspace_id: &str,
    query: QueryCollab,
  ) -> Result<EncodedCollab, AppError> {
    // Attempt to retrieve encoded collab from memory cache, falling back to disk cache if necessary.
    if let Some(encoded_collab) = self.mem_cache.get_encode_collab(&query.object_id).await {
      event!(
        Level::DEBUG,
        "Did get encode collab:{} from cache",
        query.object_id
      );
      return Ok(encoded_collab);
    }

    // Retrieve from disk cache as fallback. After retrieval, the value is inserted into the memory cache.
    let object_id = query.object_id.clone();
    let expiration_secs = cache_exp_secs_from_collab_type(&query.collab_type);
    let encode_collab = self
      .disk_cache
      .get_collab_encoded_from_disk(workspace_id, query)
      .await?;

    // spawn a task to insert the encoded collab into the memory cache
    let cloned_encode_collab = encode_collab.clone();
    let mem_cache = self.mem_cache.clone();
    let timestamp = chrono::Utc::now().timestamp();
    tokio::spawn(async move {
      mem_cache
        .insert_encode_collab(&object_id, cloned_encode_collab, timestamp, expiration_secs)
        .await;
    });
    Ok(encode_collab)
  }

  /// Batch get the encoded collab data from the cache.
  /// returns a hashmap of the object_id to the encoded collab data.
  pub async fn batch_get_encode_collab<T: Into<QueryCollab>>(
    &self,
    workspace_id: &str,
    queries: Vec<T>,
  ) -> HashMap<String, QueryCollabResult> {
    let queries = queries.into_iter().map(Into::into).collect::<Vec<_>>();
    let mut results = HashMap::new();
    // 1. Processes valid queries against the in-memory cache to retrieve cached values.
    //    - Queries not found in the cache are earmarked for disk retrieval.
    let (disk_queries, values_from_mem_cache): (Vec<_>, HashMap<_, _>) = stream::iter(queries)
      .then(|params| async move {
        match self
          .mem_cache
          .get_encode_collab_data(&params.object_id)
          .await
        {
          None => Either::Left(params),
          Some(data) => Either::Right((
            params.object_id.clone(),
            QueryCollabResult::Success {
              encode_collab_v1: data,
            },
          )),
        }
      })
      .collect::<Vec<_>>()
      .await
      .into_iter()
      .partition_map(|either| either);
    results.extend(values_from_mem_cache);

    // 2. Retrieves remaining values from the disk cache for queries not satisfied by the memory cache.
    //    - These values are then merged into the final result set.
    let values_from_disk_cache = self
      .disk_cache
      .batch_get_collab(workspace_id, disk_queries)
      .await;
    results.extend(values_from_disk_cache);
    results
  }

  /// Insert the encoded collab data into the cache.
  /// The data is inserted into both the memory and disk cache.
  pub async fn insert_encode_collab_data(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> Result<(), AppError> {
    let collab_type = params.collab_type.clone();
    let object_id = params.object_id.clone();
    let encode_collab_data = params.encoded_collab_v1.clone();
    let s3 = self.disk_cache.s3_client();
    CollabDiskCache::upsert_collab_with_transaction(
      workspace_id,
      uid,
      params,
      transaction,
      s3,
      self.s3_collab_threshold,
      &self.metrics,
    )
    .await?;

    // when the data is written to the disk cache but fails to be written to the memory cache
    // we log the error and continue.
    self.cache_collab(object_id, collab_type, encode_collab_data);
    Ok(())
  }

  fn cache_collab(&self, object_id: String, collab_type: CollabType, encode_collab_data: Bytes) {
    let mem_cache = self.mem_cache.clone();
    tokio::spawn(async move {
      if let Err(err) = mem_cache
        .insert_encode_collab_data(
          &object_id,
          &encode_collab_data,
          chrono::Utc::now().timestamp(),
          Some(cache_exp_secs_from_collab_type(&collab_type)),
        )
        .await
      {
        error!(
          "Failed to insert encode collab into memory cache: {:?}",
          err
        );
      }
    });
  }

  pub async fn insert_encode_collab_to_disk(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
  ) -> Result<(), AppError> {
    let p = params.clone();
    self
      .disk_cache
      .upsert_collab(workspace_id, uid, params)
      .await?;
    self.cache_collab(p.object_id, p.collab_type, p.encoded_collab_v1);
    Ok(())
  }

  pub async fn delete_collab(&self, workspace_id: &str, object_id: &str) -> Result<(), AppError> {
    self.mem_cache.remove_encode_collab(object_id).await?;
    self
      .disk_cache
      .delete_collab(workspace_id, object_id)
      .await?;
    Ok(())
  }

  pub async fn is_exist(&self, workspace_id: &str, oid: &str) -> Result<bool, AppError> {
    if let Ok(value) = self.mem_cache.is_exist(oid).await {
      if value {
        return Ok(value);
      }
    }

    let is_exist = self.disk_cache.is_exist(workspace_id, oid).await?;
    Ok(is_exist)
  }

  pub async fn batch_insert_collab(
    &self,
    records: Vec<PendingCollabWrite>,
  ) -> Result<(), AppError> {
    let mem_cache_params: Vec<_> = records
      .iter()
      .map(|r| {
        (
          r.params.object_id.clone(),
          r.params.encoded_collab_v1.clone(),
          cache_exp_secs_from_collab_type(&r.params.collab_type),
        )
      })
      .collect();

    self.disk_cache.batch_insert_collab(records).await?;

    // We'll update cache in the background. The reason is that Redis
    // doesn't have a good way to do batch insert, so we'll do it one
    // by one which may take time if there are many records.
    //
    // Most of the code doesn't rely on the cache being the only source
    // of truth and accepts possibility that its update may fail.
    let mem_cache = self.mem_cache.clone();
    tokio::spawn(async move {
      let now = chrono::Utc::now().timestamp();
      for (oid, data, expire) in mem_cache_params {
        if let Err(err) = mem_cache
          .insert_encode_collab_data(&oid, &data, now, Some(expire))
          .await
        {
          error!(
            "Failed to insert collab `{}` into memory cache: {}",
            oid, err
          );
        }
      }
    });

    Ok(())
  }
}
