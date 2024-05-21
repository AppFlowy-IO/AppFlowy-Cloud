use std::collections::HashMap;
use std::ops::DerefMut;
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use collab_entity::CollabType;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::Mutex;
use tokio::time::{interval, sleep, sleep_until, Instant};
use tracing::{error, instrument, trace, warn};

use crate::collab::cache::CollabCache;
use app_error::AppError;
use database_entity::dto::{CollabParams, QueryCollab, QueryCollabResult};

use crate::collab::queue_redis_ops::{
  get_pending_meta, remove_all_pending_meta, remove_pending_meta, storage_cache_key, PendingWrite,
  WritePriority, PENDING_WRITE_META_EXPIRE_SECS,
};
use crate::collab::RedisSortedSet;
use crate::metrics::CollabMetrics;
use crate::state::RedisConnectionManager;

type PendingWriteSet = Arc<RedisSortedSet>;
#[derive(Clone)]
pub struct StorageQueue {
  collab_cache: CollabCache,
  connection_manager: RedisConnectionManager,
  pending_write_set: PendingWriteSet,
  pending_id_counter: Arc<AtomicI64>,
  total_queue_collab_count: Arc<AtomicI64>,
  success_queue_collab_count: Arc<AtomicI64>,
}

pub const REDIS_PENDING_WRITE_QUEUE: &str = "collab_pending_write_queue_v0";

impl StorageQueue {
  pub fn new(
    collab_cache: CollabCache,
    connection_manager: RedisConnectionManager,
    queue_name: &str,
  ) -> Self {
    Self::new_with_metrics(collab_cache, connection_manager, queue_name, None)
  }

  pub fn new_with_metrics(
    collab_cache: CollabCache,
    connection_manager: RedisConnectionManager,
    queue_name: &str,
    metrics: Option<Arc<CollabMetrics>>,
  ) -> Self {
    let next_duration = Arc::new(Mutex::new(Duration::from_secs(1)));
    let pending_id_counter = Arc::new(AtomicI64::new(0));
    let pending_write_set = Arc::new(RedisSortedSet::new(connection_manager.clone(), queue_name));

    let total_queue_collab_count = Arc::new(AtomicI64::new(0));
    let success_queue_collab_count = Arc::new(AtomicI64::new(0));

    // Spawns a task that periodically writes pending collaboration objects to the database.
    spawn_period_write(
      next_duration.clone(),
      collab_cache.clone(),
      connection_manager.clone(),
      pending_write_set.clone(),
      metrics.clone(),
      total_queue_collab_count.clone(),
      success_queue_collab_count.clone(),
    );

    spawn_period_check_pg_conn_count(collab_cache.pg_pool().clone(), next_duration);

    Self {
      collab_cache,
      connection_manager,
      pending_write_set,
      pending_id_counter,
      total_queue_collab_count,
      success_queue_collab_count,
    }
  }

  /// Enqueues a object for deferred processing. High priority writes are processed before low priority writes.
  ///
  /// adds a write task to a pending queue, which is periodically flushed by another task that batches
  /// and writes the queued collaboration objects to a PostgreSQL database.
  ///
  /// This data is stored temporarily in the `collab_cache` and is intended for later persistent storage
  /// in the database. It can also be retrieved during subsequent calls in the [CollabStorageImpl::get_encode_collab]
  /// to enhance performance and reduce database reads.
  ///
  #[instrument(level = "trace", skip_all)]
  pub async fn push(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: &CollabParams,
    priority: WritePriority,
  ) -> Result<(), AppError> {
    trace!("queuing {} object to pending write queue", params.object_id,);
    // TODO(nathan): compress the data before storing it in Redis
    self
      .collab_cache
      .insert_encode_collab_data_in_mem(params)
      .await?;

    let seq = self
      .pending_id_counter
      .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let pending_write = PendingWrite {
      object_id: params.object_id.clone(),
      seq,
      data_len: params.encoded_collab_v1.len(),
      priority,
    };

    let pending_meta = PendingWriteMeta {
      uid: *uid,
      workspace_id: workspace_id.to_string(),
      object_id: params.object_id.clone(),
      collab_type: params.collab_type.clone(),
    };

    self
      .total_queue_collab_count
      .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // If the queueing fails, write the data to the database immediately
    if let Err(err) = self
      .queue_pending(params, pending_write, pending_meta)
      .await
    {
      error!(
        "Failed to queue pending write for object {}: {:?}",
        params.object_id, err
      );

      let mut transaction = self
        .collab_cache
        .pg_pool()
        .begin()
        .await
        .context("acquire transaction to upsert collab")
        .map_err(AppError::from)?;
      self
        .collab_cache
        .insert_encode_collab_data(workspace_id, uid, params, &mut transaction)
        .await?;
      transaction
        .commit()
        .await
        .context("fail to commit the transaction to upsert collab")
        .map_err(AppError::from)?;
    } else {
      self
        .success_queue_collab_count
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
      trace!(
        "did queue {}:{} object for deferred writing to disk",
        params.object_id,
        seq
      );
    }

    Ok(())
  }

  #[cfg(debug_assertions)]
  pub async fn clear(&self) -> Result<(), AppError> {
    self.pending_write_set.clear().await?;
    remove_all_pending_meta(self.connection_manager.clone()).await?;
    Ok(())
  }

  #[inline]
  async fn queue_pending(
    &self,
    params: &CollabParams,
    pending_write: PendingWrite,
    pending_write_meta: PendingWriteMeta,
  ) -> Result<(), anyhow::Error> {
    const MAX_RETRIES: usize = 3;
    const BASE_DELAY_MS: u64 = 200;
    const BACKOFF_FACTOR: u64 = 2;

    // these serialization seems very fast, so we don't need to worry about the performance and no
    // need to use spawn_blocking or block_in_place
    let pending_write_data = serde_json::to_vec(&pending_write)?;
    let pending_write_meta_data = serde_json::to_vec(&pending_write_meta)?;

    let key = storage_cache_key(&params.object_id, params.encoded_collab_v1.len());
    let mut conn = self.connection_manager.clone();
    for attempt in 0..MAX_RETRIES {
      let mut pipe = redis::pipe();
      // Prepare the pipeline with both commands
      // 1. ZADD to add the pending write to the queue
      // 2. SETEX to add the pending metadata to the cache
      pipe
        // .atomic()
        .cmd("ZADD")
        .arg(self.pending_write_set.queue_name())
        .arg(pending_write.score())
        .arg(&pending_write_data)
        .ignore()
        .cmd("SETEX")
        .arg(&key)
        .arg(PENDING_WRITE_META_EXPIRE_SECS)
        .arg(&pending_write_meta_data)
        .ignore();

      match pipe.query_async::<_, ()>(&mut conn).await {
        Ok(_) => return Ok(()),
        Err(e) => {
          if attempt == MAX_RETRIES - 1 {
            return Err(e.into());
          }

          // 200ms, 400ms, 800ms
          let delay = BASE_DELAY_MS * BACKOFF_FACTOR.pow(attempt as u32);
          sleep(Duration::from_millis(delay)).await;
        },
      }
    }
    Err(anyhow!("Failed to execute redis pipeline after retries"))
  }
}

/// Spawn a task that periodically checks the number of active connections in the PostgreSQL pool
/// It aims to adjust the write interval based on the number of active connections.
fn spawn_period_check_pg_conn_count(pg_pool: PgPool, next_duration: Arc<Mutex<Duration>>) {
  let mut interval = interval(tokio::time::Duration::from_secs(5));
  tokio::spawn(async move {
    loop {
      interval.tick().await;
      // these range values are arbitrary and can be adjusted as needed
      match pg_pool.size() {
        0..=40 => {
          *next_duration.lock().await = Duration::from_secs(1);
        },
        _ => {
          *next_duration.lock().await = Duration::from_secs(2);
        },
      }
    }
  });
}

fn spawn_period_write(
  next_duration: Arc<Mutex<Duration>>,
  collab_cache: CollabCache,
  connection_manager: RedisConnectionManager,
  pending_write_set: PendingWriteSet,
  metrics: Option<Arc<CollabMetrics>>,
  total_queue_collab_count: Arc<AtomicI64>,
  success_queue_collab_count: Arc<AtomicI64>,
) {
  let total_write_count = Arc::new(AtomicI64::new(0));
  let success_write_count = Arc::new(AtomicI64::new(0));
  tokio::spawn(async move {
    loop {
      // The next_duration will be changed by spawn_period_check_pg_conn_count. When the number of
      // active connections is high, the interval will be longer.
      let instant = Instant::now() + *next_duration.lock().await;
      sleep_until(instant).await;

      if let Some(metrics) = metrics.as_ref() {
        metrics.record_write_collab(
          success_write_count.load(std::sync::atomic::Ordering::Relaxed),
          total_write_count.load(std::sync::atomic::Ordering::Relaxed),
        );

        metrics.record_queue_collab(
          success_queue_collab_count.load(std::sync::atomic::Ordering::Relaxed),
          total_queue_collab_count.load(std::sync::atomic::Ordering::Relaxed),
        );
      }

      let chunk_keys = consume_pending_write(&pending_write_set, 30, 10).await;
      if chunk_keys.is_empty() {
        continue;
      }

      for keys in chunk_keys {
        trace!(
          "start writing {} pending collaboration data to disk",
          keys.len()
        );
        let cloned_collab_cache = collab_cache.clone();
        let mut cloned_connection_manager = connection_manager.clone();
        let cloned_total_write_count = total_write_count.clone();
        let cloned_total_success_write_count = success_write_count.clone();

        tokio::spawn(async move {
          if let Ok(metas) = get_pending_meta(&keys, &mut cloned_connection_manager).await {
            if metas.is_empty() {
              error!("the pending write keys is not empty, but metas is empty");
              return;
            }

            match retry_write_pending_to_disk(&cloned_collab_cache, metas).await {
              Ok(success_result) => {
                #[cfg(debug_assertions)]
                tracing::info!("success write pending: {:?}", keys,);

                trace!("{:?}", success_result);
                cloned_total_write_count.fetch_add(
                  success_result.expected as i64,
                  std::sync::atomic::Ordering::Relaxed,
                );
                cloned_total_success_write_count.fetch_add(
                  success_result.success as i64,
                  std::sync::atomic::Ordering::Relaxed,
                );
              },
              Err(err) => error!("{:?}", err),
            }
            // Remove pending metadata from Redis even if some records fail to write to disk after retries.
            // Records that fail repeatedly are considered potentially corrupt or invalid.
            let _ = remove_pending_meta(&keys, &mut cloned_connection_manager).await;
          }
        });
      }
    }
  });
}

async fn retry_write_pending_to_disk(
  collab_cache: &CollabCache,
  mut metas: Vec<PendingWriteMeta>,
) -> Result<WritePendingResult, AppError> {
  const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(2)];

  let expected = metas.len();
  let mut successes = Vec::with_capacity(metas.len());

  for &delay in RETRY_DELAYS.iter() {
    match write_pending_to_disk(&metas, collab_cache).await {
      Ok(success_write_objects) => {
        if !success_write_objects.is_empty() {
          successes.extend_from_slice(&success_write_objects);
          metas.retain(|meta| !success_write_objects.contains(&meta.object_id));
        }

        // If there are no more metas to process, return the successes
        if metas.is_empty() {
          return Ok(WritePendingResult {
            expected,
            success: successes.len(),
            fail: 0,
          });
        }
      },
      Err(err) => {
        warn!(
          "Error writing to disk: {:?}, retrying after {:?}",
          err, delay
        );
      },
    }

    // Only sleep if there are more attempts left
    if !metas.is_empty() {
      sleep(delay).await;
    }
  }

  if expected >= successes.len() {
    Ok(WritePendingResult {
      expected,
      success: successes.len(),
      fail: expected - successes.len(),
    })
  } else {
    Err(AppError::Internal(anyhow!(
      "the len of expected is less than success"
    )))
  }
}

#[derive(Debug)]
struct WritePendingResult {
  expected: usize,
  success: usize,
  #[allow(dead_code)]
  fail: usize,
}

async fn write_pending_to_disk(
  pending_metas: &[PendingWriteMeta],
  collab_cache: &CollabCache,
) -> Result<Vec<String>, AppError> {
  let mut success_write_objects = Vec::with_capacity(pending_metas.len());
  // Convert pending metadata into query parameters for batch fetching
  let queries = pending_metas
    .iter()
    .map(QueryCollab::from)
    .collect::<Vec<_>>();

  // Retrieve encoded collaboration data in batch
  let results = collab_cache.batch_get_encode_collab(queries).await;

  // Create a mapping from object IDs to their corresponding metadata
  let meta_map = pending_metas
    .iter()
    .map(|meta| (meta.object_id.clone(), meta))
    .collect::<HashMap<_, _>>();

  // Prepare collaboration data for writing to the database
  let records = results
    .into_iter()
    .filter_map(|(object_id, result)| {
      if let QueryCollabResult::Success { encode_collab_v1 } = result {
        meta_map.get(&object_id).map(|meta| PendingWriteData {
          uid: meta.uid,
          workspace_id: meta.workspace_id.clone(),
          object_id: meta.object_id.clone(),
          collab_type: meta.collab_type.clone(),
          encode_collab_v1,
        })
      } else {
        None
      }
    })
    .collect::<Vec<_>>();

  // Start a database transaction
  let mut transaction = collab_cache
    .pg_pool()
    .begin()
    .await
    .context("Failed to acquire transaction for writing pending collaboration data")
    .map_err(AppError::from)?;

  // Insert each record into the database within the transaction context
  for (index, record) in records.into_iter().enumerate() {
    let params = CollabParams {
      object_id: record.object_id.clone(),
      collab_type: record.collab_type,
      encoded_collab_v1: record.encode_collab_v1,
    };
    let savepoint_name = format!("sp_{}", index);

    // using savepoint to rollback the transaction if the insert fails
    sqlx::query(&format!("SAVEPOINT {}", savepoint_name))
      .execute(transaction.deref_mut())
      .await?;
    if let Err(_err) = collab_cache
      .insert_encode_collab_in_disk(&record.workspace_id, &record.uid, params, &mut transaction)
      .await
    {
      sqlx::query(&format!("ROLLBACK TO SAVEPOINT {}", savepoint_name))
        .execute(transaction.deref_mut())
        .await?;
    } else {
      success_write_objects.push(record.object_id);
    }
  }

  // Commit the transaction to finalize all writes
  transaction
    .commit()
    .await
    .context("Failed to commit the transaction for pending collaboration data")
    .map_err(AppError::from)?;
  Ok(success_write_objects)
}

const MAXIMUM_CHUNK_SIZE: usize = 5 * 1024 * 1024;
#[inline]
pub async fn consume_pending_write(
  pending_write_set: &PendingWriteSet,
  maximum_consume_item: usize,
  num_of_item_each_chunk: usize,
) -> Vec<Vec<String>> {
  let mut chunks = Vec::new();
  let mut current_chunk = Vec::with_capacity(maximum_consume_item);
  let mut current_chunk_data_size = 0;

  if let Ok(items) = pending_write_set.pop(maximum_consume_item).await {
    #[cfg(debug_assertions)]
    if !items.is_empty() {
      trace!("Consuming {} pending write items", items.len());
    }

    for item in items {
      let item_size = item.data_len;
      // Check if adding this item would exceed the maximum chunk size or item limit
      if current_chunk_data_size + item_size > MAXIMUM_CHUNK_SIZE
        || current_chunk.len() >= num_of_item_each_chunk
      {
        if !current_chunk.is_empty() {
          chunks.push(std::mem::take(&mut current_chunk));
        }
        current_chunk_data_size = 0;
      }

      // Add the item to the current batch and update the batch size
      current_chunk.push(item);
      current_chunk_data_size += item_size;
    }
  }

  if !current_chunk.is_empty() {
    chunks.push(current_chunk);
  }
  // Convert each batch of items into a batch of keys
  chunks
    .into_iter()
    .map(|batch| {
      batch
        .into_iter()
        .map(|pending| storage_cache_key(&pending.object_id, pending.data_len))
        .collect()
    })
    .collect()
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub struct PendingWriteMeta {
  pub uid: i64,
  pub workspace_id: String,
  pub object_id: String,
  pub collab_type: CollabType,
}

impl From<&PendingWriteMeta> for QueryCollab {
  fn from(meta: &PendingWriteMeta) -> Self {
    QueryCollab {
      object_id: meta.object_id.clone(),
      collab_type: meta.collab_type.clone(),
    }
  }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub struct PendingWriteData {
  pub uid: i64,
  pub workspace_id: String,
  pub object_id: String,
  pub collab_type: CollabType,
  pub encode_collab_v1: Vec<u8>,
}

impl From<PendingWriteData> for CollabParams {
  fn from(data: PendingWriteData) -> Self {
    CollabParams {
      object_id: data.object_id,
      collab_type: data.collab_type,
      encoded_collab_v1: data.encode_collab_v1,
    }
  }
}
