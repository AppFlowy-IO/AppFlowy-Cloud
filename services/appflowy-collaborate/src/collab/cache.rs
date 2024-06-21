use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use futures_util::{stream, StreamExt};
use itertools::{Either, Itertools};
use sqlx::{PgPool, Transaction};
use tracing::{error, event, Level};

use app_error::AppError;
use database::collab::CollabMetadata;
use database_entity::dto::{CollabParams, QueryCollab, QueryCollabResult};

use crate::collab::disk_cache::CollabDiskCache;
use crate::collab::mem_cache::CollabMemCache;
use crate::state::RedisConnectionManager;

#[derive(Clone)]
pub struct CollabCache {
  disk_cache: CollabDiskCache,
  mem_cache: CollabMemCache,
  success_attempts: Arc<AtomicU64>,
  total_attempts: Arc<AtomicU64>,
}

impl CollabCache {
  pub fn new(redis_conn_manager: RedisConnectionManager, pg_pool: PgPool) -> Self {
    let mem_cache = CollabMemCache::new(redis_conn_manager.clone());
    let disk_cache = CollabDiskCache::new(pg_pool.clone());
    Self {
      disk_cache,
      mem_cache,
      success_attempts: Arc::new(AtomicU64::new(0)),
      total_attempts: Arc::new(AtomicU64::new(0)),
    }
  }

  pub async fn get_collab_meta(
    &self,
    object_id: &str,
    collab_type: &CollabType,
  ) -> Result<CollabMetadata, AppError> {
    match self.mem_cache.get_collab_meta(object_id).await {
      Ok(meta) => Ok(meta),
      Err(_) => {
        let row = self
          .disk_cache
          .get_collab_meta(object_id, collab_type)
          .await?;
        let meta = CollabMetadata {
          object_id: row.oid,
          workspace_id: row.workspace_id.to_string(),
        };

        // Spawn a background task to insert the collaboration metadata into the memory cache.
        let cloned_meta = meta.clone();
        let mem_cache = self.mem_cache.clone();
        tokio::spawn(async move {
          if let Err(err) = mem_cache.insert_collab_meta(cloned_meta).await {
            error!("{:?}", err);
          }
        });
        Ok(meta)
      },
    }
  }

  pub async fn get_encode_collab(
    &self,
    uid: &i64,
    query: QueryCollab,
  ) -> Result<EncodedCollab, AppError> {
    self.total_attempts.fetch_add(1, Ordering::Relaxed);
    // Attempt to retrieve encoded collab from memory cache, falling back to disk cache if necessary.
    if let Some(encoded_collab) = self.mem_cache.get_encode_collab(&query.object_id).await {
      event!(
        Level::DEBUG,
        "Did get encode collab:{} from cache",
        query.object_id
      );
      self.success_attempts.fetch_add(1, Ordering::Relaxed);
      return Ok(encoded_collab);
    }

    // Retrieve from disk cache as fallback. After retrieval, the value is inserted into the memory cache.
    let object_id = query.object_id.clone();
    let encode_collab = self
      .disk_cache
      .get_collab_encoded_from_disk(uid, query)
      .await?;

    // spawn a task to insert the encoded collab into the memory cache
    let cloned_encode_collab = encode_collab.clone();
    let mem_cache = self.mem_cache.clone();
    let timestamp = chrono::Utc::now().timestamp();
    tokio::spawn(async move {
      mem_cache
        .insert_encode_collab(&object_id, cloned_encode_collab, timestamp)
        .await;
    });
    Ok(encode_collab)
  }

  /// Batch get the encoded collab data from the cache.
  /// returns a hashmap of the object_id to the encoded collab data.
  pub async fn batch_get_encode_collab<T: Into<QueryCollab>>(
    &self,
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
    let values_from_disk_cache = self.disk_cache.batch_get_collab(disk_queries).await;
    results.extend(values_from_disk_cache);
    results
  }

  /// Insert the encoded collab data into the cache.
  /// The data is inserted into both the memory and disk cache.
  pub async fn insert_encode_collab_data(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: &CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> Result<(), AppError> {
    let object_id = params.object_id.clone();
    let encode_collab_data = params.encoded_collab_v1.clone();
    self
      .disk_cache
      .upsert_collab_with_transaction(workspace_id, uid, params, transaction)
      .await?;

    // when the data is written to the disk cache but fails to be written to the memory cache
    // we log the error and continue.
    if let Err(err) = self
      .mem_cache
      .insert_encode_collab_data(
        &object_id,
        &encode_collab_data,
        chrono::Utc::now().timestamp(),
      )
      .await
    {
      error!(
        "Failed to insert encode collab into memory cache: {:?}",
        err
      );
    }

    Ok(())
  }

  pub async fn get_encode_collab_from_disk(
    &self,
    uid: &i64,
    query: QueryCollab,
  ) -> Result<EncodedCollab, AppError> {
    let encode_collab = self
      .disk_cache
      .get_collab_encoded_from_disk(uid, query)
      .await?;
    Ok(encode_collab)
  }

  pub async fn insert_encode_collab_in_disk(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> Result<(), AppError> {
    self
      .disk_cache
      .upsert_collab_with_transaction(workspace_id, uid, &params, transaction)
      .await?;
    Ok(())
  }

  /// Insert the encoded collab data into the memory cache.
  pub async fn insert_encode_collab_data_in_mem(
    &self,
    params: &CollabParams,
  ) -> Result<(), AppError> {
    let timestamp = chrono::Utc::now().timestamp();
    self
      .mem_cache
      .insert_encode_collab_data(&params.object_id, &params.encoded_collab_v1, timestamp)
      .await
      .map_err(|err| AppError::Internal(err.into()))?;
    Ok(())
  }

  pub fn query_state(&self) -> QueryState {
    let successful_attempts = self.success_attempts.load(Ordering::Relaxed);
    let total_attempts = self.total_attempts.load(Ordering::Relaxed);
    QueryState {
      total_attempts,
      success_attempts: successful_attempts,
    }
  }

  pub async fn delete_collab(&self, object_id: &str) -> Result<(), AppError> {
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

pub struct QueryState {
  pub total_attempts: u64,
  pub success_attempts: u64,
}
