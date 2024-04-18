use crate::biz::collab::cache::CollabCache;
use crate::biz::collab::queue_redis_ops::{
  get_pending_meta, get_remaining_pending_write, insert_pending_meta, remove_all_pending_meta,
  remove_pending_meta, storage_cache_key,
};

use crate::state::RedisConnectionManager;
use anyhow::{anyhow, Context};
use app_error::AppError;
use collab_entity::CollabType;
use database_entity::dto::{CollabParams, QueryCollab, QueryCollabResult};
use serde::{Deserialize, Serialize};

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::ops::DerefMut;

use sqlx::PgPool;
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{interval, sleep, sleep_until, Instant};
use tracing::{error, info, trace, warn};

/// The maximum size of a batch payload in bytes.
const MAXIMUM_BATCH_PAYLOAD_SIZE: usize = 2 * 1024 * 1024;
type PendingWriteHeap = Arc<Mutex<BinaryHeap<PendingWrite>>>;
#[derive(Clone)]
pub struct StorageQueue {
  collab_cache: CollabCache,
  connection_manager: RedisConnectionManager,
  pending_write: PendingWriteHeap,
  pending_id_counter: Arc<AtomicI64>,
}

impl StorageQueue {
  pub fn new(collab_cache: CollabCache, connection_manager: RedisConnectionManager) -> Self {
    let next_duration = Arc::new(Mutex::new(Duration::from_secs(2)));
    let pending_id_counter = Arc::new(AtomicI64::new(0));
    let pending_write = Arc::new(Mutex::new(BinaryHeap::new()));

    // spawn a task that fetches remaining pending writes from the Redis cache.
    //
    spawn_fetch_remaining_pending_write(
      connection_manager.clone(),
      pending_write.clone(),
      pending_id_counter.clone(),
    );

    // Spawns a task that periodically writes pending collaboration objects to the database.
    spawn_period_write(
      next_duration.clone(),
      collab_cache.clone(),
      connection_manager.clone(),
      pending_write.clone(),
    );

    spawn_period_check_pg_conn_count(collab_cache.pg_pool().clone(), next_duration);

    Self {
      collab_cache,
      connection_manager,
      pending_write,
      pending_id_counter,
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
  pub async fn push(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: &CollabParams,
    priority: WritePriority,
  ) -> Result<(), AppError> {
    trace!(
      "Queueing {} object for deferred writing to disk",
      params.object_id
    );
    self
      .collab_cache
      .insert_encode_collab_data_in_mem(params)
      .await?;

    self
      .queue_pending(&params.object_id, params.encoded_collab_v1.len(), priority)
      .await;
    insert_pending_meta(
      uid,
      workspace_id,
      &params.object_id,
      params.encoded_collab_v1.len(),
      &params.collab_type,
      self.connection_manager.clone(),
    )
    .await?;

    Ok(())
  }

  #[cfg(debug_assertions)]
  pub async fn clear(&self) -> Result<(), AppError> {
    self.pending_write.lock().await.clear();
    remove_all_pending_meta(self.connection_manager.clone()).await?;
    Ok(())
  }

  #[inline]
  async fn queue_pending(&self, object_id: &str, size: usize, priority: WritePriority) {
    let pending = PendingWrite {
      object_id: object_id.to_string(),
      seq: self
        .pending_id_counter
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
      data_len: size,
      priority,
    };
    self.pending_write.lock().await.push(pending);
  }
}

/// Initializes and fetches any pending writes from the Redis cache at server startup and populates
/// them into the pending write heap.
///
/// This function assumes that the cache data in Redis is designed to be long-lived, with each cached
/// item set to expire after [PENDING_WRITE_EXPIRE_IN_SECS]. Given this configuration, it is safe to attempt to fetch all
/// extant pending writes from the cache upon initialization.
///
fn spawn_fetch_remaining_pending_write(
  mut connection_manager: RedisConnectionManager,
  pending_heap: PendingWriteHeap,
  counter: Arc<AtomicI64>,
) {
  tokio::task::spawn(async move {
    if let Ok(records) = get_remaining_pending_write(&counter, &mut connection_manager).await {
      info!("Fetched {} remaining pending writes", records.len());
      pending_heap
        .lock()
        .await
        .extend(records.into_iter().map(|record| PendingWrite {
          object_id: record.object_id,
          seq: counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
          data_len: record.data_len,
          priority: record.priority,
        }));
    }
  });
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
        0..=20 => {
          *next_duration.lock().await = Duration::from_secs(2);
        },
        21..=40 => {
          *next_duration.lock().await = Duration::from_secs(3);
        },
        _ => {
          *next_duration.lock().await = Duration::from_secs(5);
        },
      }
    }
  });
}

fn spawn_period_write(
  next_duration: Arc<Mutex<Duration>>,
  collab_cache: CollabCache,
  mut connection_manager: RedisConnectionManager,
  pending_heap: PendingWriteHeap,
) {
  tokio::spawn(async move {
    loop {
      // The next_duration will be changed by spawn_period_check_pg_conn_count. When the number of
      // active connections is high, the interval will be longer.
      let instant = Instant::now() + *next_duration.lock().await;
      sleep_until(instant).await;

      let keys = consume_pending_write_keys(&pending_heap, MAXIMUM_BATCH_PAYLOAD_SIZE).await;
      if let Ok(metas) = get_pending_meta(&keys, &mut connection_manager).await {
        match retry_write_pending_to_disk(&collab_cache, metas).await {
          Ok(_) => {},
          Err(err) => {
            error!("Failed to write pending collaboration data: {:?}", err);
          },
        }

        // Remove pending metadata from Redis even if some records fail to write to disk after retries.
        // Records that fail repeatedly are considered potentially corrupt or invalid.
        let _ = remove_pending_meta(&keys, &mut connection_manager).await;
      }
    }
  });
}

async fn retry_write_pending_to_disk(
  collab_cache: &CollabCache,
  mut metas: Vec<PendingWriteMeta>,
) -> Result<(), AppError> {
  if metas.is_empty() {
    return Ok(());
  }
  const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(3),
    Duration::from_secs(5),
  ];

  let expected_len = metas.len();
  let mut successes = Vec::with_capacity(metas.len());

  for &delay in RETRY_DELAYS.iter() {
    match write_pending_to_disk(&metas, collab_cache).await {
      Ok(success_write_objects) => {
        if cfg!(debug_assertions) {
          info!("Success wrote {:?} objects to disk", success_write_objects);
        }
        successes.extend_from_slice(&success_write_objects);
        metas.retain(|meta| !success_write_objects.contains(&meta.object_id));

        // If there are no more metas to process, return the successes
        if metas.is_empty() {
          return Ok(());
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

  error!(
    "expect {}, but success:{}, fail:{}",
    expected_len,
    successes.len(),
    expected_len - successes.len()
  );

  Err(AppError::Internal(anyhow!("Failed after multiple retries")))
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
    if let Err(err) = collab_cache
      .insert_encode_collab_in_disk(&record.workspace_id, &record.uid, params, &mut transaction)
      .await
    {
      trace!(
        "Failed to write {} data to disk: {:?}, rollback to save point",
        record.object_id,
        err
      );
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

#[inline]
async fn consume_pending_write_keys(
  pending_heap: &PendingWriteHeap,
  maximum_payload_size: usize,
) -> Vec<String> {
  const NUM_ITEMS: usize = 5;
  let mut pending_lock = pending_heap.lock().await;
  let mut write_items = Vec::with_capacity(NUM_ITEMS);
  while let Some(item) = pending_lock.pop() {
    write_items.push(item);
    if write_items.iter().map(|v| v.data_len).sum::<usize>() > maximum_payload_size {
      break;
    }
    if write_items.len() >= NUM_ITEMS {
      break;
    }
  }
  drop(pending_lock);

  write_items
    .iter()
    .map(|pending| storage_cache_key(&pending.object_id, pending.data_len))
    .collect()
}

#[derive(Clone)]
pub(crate) struct PendingWrite {
  pub object_id: String,
  pub seq: i64,
  pub data_len: usize,
  pub priority: WritePriority,
}

#[derive(Clone)]
pub enum WritePriority {
  High,
  Low,
}

impl Eq for PendingWrite {}
impl PartialEq for PendingWrite {
  fn eq(&self, other: &Self) -> bool {
    self.object_id == other.object_id
  }
}

impl Ord for PendingWrite {
  fn cmp(&self, other: &Self) -> Ordering {
    match (&self.priority, &other.priority) {
      (WritePriority::High, WritePriority::Low) => Ordering::Greater,
      (WritePriority::Low, WritePriority::High) => Ordering::Less,
      _ => {
        // Assuming lower seq is higher priority
        other.seq.cmp(&self.seq)
      },
    }
  }
}

impl PartialOrd for PendingWrite {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
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
