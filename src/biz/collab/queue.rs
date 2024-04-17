use crate::biz::collab::cache::CollabCache;
use crate::biz::collab::queue_redis_ops::{
  get_pending_meta, get_remaining_pending_write, insert_pending_meta, remove_pending_meta,
  storage_cache_key,
};

use crate::state::RedisConnectionManager;
use anyhow::Context;
use app_error::AppError;
use collab_entity::CollabType;
use database_entity::dto::{CollabParams, QueryCollab, QueryCollabResult};
use serde::{Deserialize, Serialize};

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

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
  next_duration: Arc<Mutex<Duration>>,
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

    spawn_period_check_pg_conn_count(collab_cache.pg_pool().clone(), next_duration.clone());

    Self {
      collab_cache,
      connection_manager,
      pending_write,
      pending_id_counter,
      next_duration,
    }
  }

  /// Enqueues a object for deferred processing.
  ///
  /// adds a write task to a pending queue, which is periodically flushed by another task that batches
  /// and writes the queued collaboration objects to a PostgreSQL database.
  ///
  /// This data is stored temporarily in the `collab_cache` and is intended for later persistent storage
  /// in the database. It can also be retrieved during subsequent calls in the [CollabStorageImpl::get_encode_collab]
  /// to enhance performance and reduce database reads.
  pub async fn push(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: &CollabParams,
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
      .queue_pending(&params.object_id, params.encoded_collab_v1.len())
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

  #[inline]
  async fn queue_pending(&self, object_id: &str, size: usize) {
    let pending = PendingWrite {
      object_id: object_id.to_string(),
      seq: self
        .pending_id_counter
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
      data_len: size,
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
          *next_duration.lock().await = Duration::from_secs(4);
        },
        _ => {
          *next_duration.lock().await = Duration::from_secs(6);
        },
      }
    }
  });
}

/// Maximum number of retry attempts
const MAX_RETRY_ATTEMPTS: usize = 3;
/// Base duration to wait for between retries
const RETRY_BASE_DURATION: u64 = 2; // seconds

fn spawn_period_write(
  next_duration: Arc<Mutex<Duration>>,
  collab_cache: CollabCache,
  mut connection_manager: RedisConnectionManager,
  pending_heap: PendingWriteHeap,
) {
  tokio::spawn(async move {
    loop {
      let duration = next_duration.lock().await.clone();
      trace!("Next write attempt in {:?}", duration);
      let instant = Instant::now() + duration;
      sleep_until(instant).await;

      let keys = consume_pending_write_keys(&pending_heap, MAXIMUM_BATCH_PAYLOAD_SIZE).await;
      if let Ok(metas) = get_pending_meta(&keys, &mut connection_manager).await {
        match invoke_write(&collab_cache, &metas).await {
          Ok(_) => {
            trace!(
              "Successfully wrote pending {:?} to disk",
              metas.iter().map(|m| &m.object_id).collect::<Vec<_>>()
            );
          },
          Err(err) => {
            error!("Failed to write pending collaboration data: {:?}", err);
          },
        }
        let _ = remove_pending_meta(&keys, &mut connection_manager).await;
      }
    }
  });
}

async fn invoke_write(
  collab_cache: &CollabCache,
  metas: &[PendingWriteMeta],
) -> Result<(), AppError> {
  let mut retry_count = 0;
  loop {
    match write_pending_to_disk(metas, collab_cache).await {
      Ok(_) => return Ok(()),
      Err(err) if retry_count < MAX_RETRY_ATTEMPTS => {
        warn!(
          "Attempt {} failed, retrying... Error: {:?}",
          retry_count + 1,
          err
        );
        sleep(Duration::from_secs(RETRY_BASE_DURATION)).await;
        retry_count += 1;
      },
      Err(err) => {
        warn!(
          "Failed to write pending collab to disk after {} attempts: {:?}",
          MAX_RETRY_ATTEMPTS, err
        );
        return Err(err);
      },
    }
  }
}

async fn write_pending_to_disk(
  pending_metas: &[PendingWriteMeta],
  collab_cache: &CollabCache,
) -> Result<(), AppError> {
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
  for record in records {
    let params = CollabParams {
      object_id: record.object_id,
      collab_type: record.collab_type,
      encoded_collab_v1: record.encode_collab_v1,
    };
    collab_cache
      .insert_encode_collab_in_disk(&record.workspace_id, &record.uid, params, &mut transaction)
      .await?;
  }

  // Commit the transaction to finalize all writes
  transaction
    .commit()
    .await
    .context("Failed to commit the transaction for pending collaboration data")
    .map_err(AppError::from)?;

  Ok(())
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
}

impl Eq for PendingWrite {}
impl PartialEq for PendingWrite {
  fn eq(&self, other: &Self) -> bool {
    self.object_id == other.object_id
  }
}

impl Ord for PendingWrite {
  fn cmp(&self, other: &Self) -> Ordering {
    // smaller timestamp has higher priority
    self.seq.cmp(&other.seq).reverse()
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
