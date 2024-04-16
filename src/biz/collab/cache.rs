use crate::biz::collab::disk_cache::CollabDiskCache;
use crate::biz::collab::mem_cache::CollabMemCache;
use app_error::AppError;
use collab::core::collab_plugin::EncodedCollab;

use crate::state::RedisConnectionManager;

use database_entity::dto::{CollabParams, QueryCollab, QueryCollabParams, QueryCollabResult};
use futures_util::{stream, StreamExt};
use itertools::{Either, Itertools};
use sqlx::{PgPool, Transaction};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{event, Level};

#[derive(Clone)]
pub struct CollabCache {
  disk_cache: CollabDiskCache,
  mem_cache: CollabMemCache,
  hits: Arc<AtomicU64>,
  total_attempts: Arc<AtomicU64>,
}

impl CollabCache {
  pub fn new(redis_client: RedisConnectionManager, pg_pool: PgPool) -> Self {
    let mem_cache = CollabMemCache::new(redis_client.clone());
    let disk_cache = CollabDiskCache::new(pg_pool.clone());
    Self {
      disk_cache,
      mem_cache,
      hits: Arc::new(AtomicU64::new(0)),
      total_attempts: Arc::new(AtomicU64::new(0)),
    }
  }

  pub async fn get_collab_encode_data(
    &self,
    uid: &i64,
    params: QueryCollabParams,
  ) -> Result<EncodedCollab, AppError> {
    self.total_attempts.fetch_add(1, Ordering::Relaxed);
    // Attempt to retrieve encoded collab from memory cache, falling back to disk cache if necessary.
    if let Some(encoded_collab) = self.mem_cache.get_encode_collab(&params.object_id).await {
      event!(
        Level::DEBUG,
        "Get encoded collab:{} from cache",
        params.object_id
      );
      self.hits.fetch_add(1, Ordering::Relaxed);
      return Ok(encoded_collab);
    }

    // Retrieve from disk cache as fallback. After retrieval, the value is inserted into the memory cache.
    let object_id = params.object_id.clone();
    let encode_collab = self
      .disk_cache
      .get_collab_encoded_from_disk(uid, params)
      .await?;

    // spawn a task to insert the encoded collab into the memory cache
    let cloned_encode_collab = encode_collab.clone();
    let mem_cache = self.mem_cache.clone();
    let timestamp = chrono::Utc::now().timestamp();
    tokio::spawn(async move {
      mem_cache
        .insert_encode_collab(object_id, cloned_encode_collab, timestamp)
        .await;
    });
    Ok(encode_collab)
  }

  pub async fn batch_get_encode_collab(
    &self,
    uid: &i64,
    queries: Vec<QueryCollab>,
  ) -> HashMap<String, QueryCollabResult> {
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
    let values_from_disk_cache = self.disk_cache.batch_get_collab(uid, disk_queries).await;
    results.extend(values_from_disk_cache);
    results
  }

  pub async fn insert_encode_collab_data(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> Result<(), AppError> {
    let object_id = params.object_id.clone();
    let encoded_collab = params.encoded_collab_v1.clone();
    self
      .disk_cache
      .upsert_collab_with_transaction(workspace_id, uid, params, transaction)
      .await?;

    let timestamp = chrono::Utc::now().timestamp();
    let mem_cache = self.mem_cache.clone();
    tokio::spawn(async move {
      mem_cache
        .insert_encode_collab_data(object_id, encoded_collab, timestamp)
        .await;
    });

    Ok(())
  }

  pub fn get_hit_rate(&self) -> f64 {
    let hits = self.hits.load(Ordering::Relaxed) as f64;
    let total_attempts = self.total_attempts.load(Ordering::Relaxed) as f64;

    if total_attempts == 0.0 {
      0.0
    } else {
      hits / total_attempts
    }
  }

  pub async fn remove_collab(&self, object_id: &str) -> Result<(), AppError> {
    self.mem_cache.remove_encode_collab(object_id).await?;
    self.disk_cache.delete_collab(object_id).await?;
    Ok(())
  }

  pub async fn is_exist(&self, oid: &str) -> Result<bool, AppError> {
    if let Ok(value) = self.mem_cache.is_exist(oid).await {
      if value {
        return Ok(value);
      }
    }

    let is_exist = self.disk_cache.is_exist(oid).await?;
    Ok(is_exist)
  }

  pub fn pg_pool(&self) -> &sqlx::PgPool {
    &self.disk_cache.pg_pool
  }
}
