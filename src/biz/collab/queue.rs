use crate::biz::collab::cache::CollabCache;
use anyhow::Context;
use app_error::AppError;
use dashmap::DashMap;
use database_entity::dto::CollabParams;

use crate::biz::collab::storage::check_encoded_collab_data;
use sqlx::PgPool;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8};
use std::sync::{Arc, Weak};
use tokio::sync::{watch, Mutex};
use tokio::time::interval;
use tracing::{error, trace};

/// The maximum size of a batch payload in bytes.
/// This is set to 2MB
const MAXIMUM_BATCH_PAYLOAD_SIZE: usize = 1024 * 1024;

pub struct StorageQueue {
  cache: CollabCache,
  write_id_counter: AtomicU64,
  pending_collab: Arc<DashMap<String, CollabParams>>,
  pending_write: Arc<Mutex<BinaryHeap<PendingWrite>>>,
  notify: watch::Sender<WriteSignal>,
  batch_size: Arc<AtomicU8>,
  is_writing: Arc<AtomicBool>,
}

impl StorageQueue {
  pub fn new(cache: CollabCache, notify: watch::Sender<WriteSignal>) -> Self {
    let write_id_counter = AtomicU64::new(0);
    let pending_collab = Arc::new(DashMap::new());
    let pending_write = Arc::new(Mutex::new(BinaryHeap::new()));
    let batch_size = Arc::new(AtomicU8::new(5));
    let is_writing = Arc::new(AtomicBool::new(false));

    let pg_pool = cache.pg_pool().clone();
    period_check_pool_size(pg_pool.clone(), batch_size.clone());

    Self {
      cache,
      write_id_counter,
      pending_collab,
      pending_write,
      notify,
      batch_size,
      is_writing,
    }
  }

  pub async fn push(
    &self,
    uid: i64,
    workspace_id: &str,
    params: CollabParams,
  ) -> Result<(), AppError> {
    check_encoded_collab_data(&params.object_id, &params.encoded_collab_v1)?;

    // Update the memory cache with the encoded collab.
    self
      .cache
      .cache_collab_params_in_memory(params.clone())
      .await?;

    self.queue_write(uid, workspace_id, params).await;

    // notify the queue to process the batch
    let _ = self.notify.send(WriteSignal::Process);
    Ok(())
  }

  async fn queue_write(&self, uid: i64, workspace_id: &str, params: CollabParams) {
    let object_id = params.object_id.clone();
    if self.pending_collab.contains_key(&params.object_id) {
      self.pending_collab.insert(object_id, params);
      return;
    }

    let write_id = self
      .write_id_counter
      .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let item = PendingWrite {
      uid,
      workspace_id: workspace_id.to_string(),
      object_id: params.object_id.clone(),
      write_id,
      payload_size: params.encoded_collab_v1.len(),
    };
    self.pending_write.lock().await.push(item);
    self.pending_collab.insert(object_id, params);
  }

  async fn process_batch(&self) {
    if self.pending_write.lock().await.is_empty() {
      return;
    }

    if self.is_writing.load(std::sync::atomic::Ordering::SeqCst) {
      let _ = self.notify.send(WriteSignal::ProcessAfterMillis(1000));
      return;
    }
    self
      .is_writing
      .store(true, std::sync::atomic::Ordering::SeqCst);

    let batch_size = self.batch_size.load(std::sync::atomic::Ordering::Relaxed);
    let mut items = vec![];
    while let Some(item) = self.pending_write.lock().await.pop() {
      items.push(item);
      if items.iter().map(|v| v.payload_size).sum::<usize>() > MAXIMUM_BATCH_PAYLOAD_SIZE {
        break;
      }

      if items.len() >= batch_size as usize {
        break;
      }
    }
    self.pending_write.lock().await.extend(items.clone());
    if let Err(err) = self.save_to_disk(items).await {
      error!("Failed to batch write collab: {}", err);
    }

    self
      .is_writing
      .store(false, std::sync::atomic::Ordering::SeqCst);
    let _ = self.notify.send(WriteSignal::ProcessAfterMillis(1000));
  }

  async fn save_to_disk(&self, items: Vec<PendingWrite>) -> Result<(), AppError> {
    let mut success_write_items: Vec<PendingWrite> = Vec::with_capacity(items.len());
    let mut transaction = self
      .cache
      .pg_pool()
      .begin()
      .await
      .context("acquire transaction to upsert collab")
      .map_err(AppError::from)?;

    trace!("[StorageQueue]: try write {} to disk", items.len());
    for item in items {
      match self.pending_collab.get(&item.object_id).map(|v| v.clone()) {
        None => {
          error!(
            "collab object_id:{} is not found in the pending_collab",
            item.object_id
          );
          success_write_items.push(item);
        },
        Some(params) => {
          match self
            .cache
            .persist_collab_params_with_transaction(
              &item.workspace_id,
              &item.uid,
              params,
              &mut transaction,
            )
            .await
          {
            Ok(_) => {
              success_write_items.push(item);
            },
            Err(err) => {
              trace!("[StorageQueue]: fail to write collab: {}", err);
            },
          }
        },
      }
    }

    transaction
      .commit()
      .await
      .context("fail to commit the transaction to upsert collab")
      .map_err(AppError::from)?;

    for item in &success_write_items {
      self.pending_collab.remove(&item.object_id);
      self
        .pending_write
        .lock()
        .await
        .retain(|v| v.write_id != item.write_id);
    }

    trace!(
      "[StorageQueue]: write {} to disk. pending: {}",
      success_write_items.len(),
      self.pending_write.lock().await.len()
    );

    Ok(())
  }
}

pub struct StorageQueueRunner;
impl StorageQueueRunner {
  pub async fn run(queue: Weak<StorageQueue>, mut notifier: watch::Receiver<WriteSignal>) {
    loop {
      if notifier.changed().await.is_err() {
        break;
      }

      if let Some(queue) = queue.upgrade() {
        let value = notifier.borrow().clone();
        match value {
          WriteSignal::Process => {
            queue.process_batch().await;
          },
          WriteSignal::ProcessAfterMillis(millis) => {
            tokio::time::sleep(tokio::time::Duration::from_millis(millis)).await;
            queue.process_batch().await;
          },
          WriteSignal::Idle => {},
        }
      } else {
        break;
      }
    }
  }
}

fn period_check_pool_size(pg_pool: PgPool, batch_size: Arc<AtomicU8>) {
  let mut interval = interval(tokio::time::Duration::from_secs(5));
  tokio::spawn(async move {
    loop {
      interval.tick().await;
      // period check the pg state and then update the batch size
      let active_conn = pg_pool.size();
      if active_conn > 10 {
        batch_size.store(5, std::sync::atomic::Ordering::Relaxed);
      } else {
        batch_size.store(10, std::sync::atomic::Ordering::Relaxed);
      }
    }
  });
}

#[derive(Clone)]
pub enum WriteSignal {
  Idle,
  Process,
  ProcessAfterMillis(u64),
}

#[derive(Clone)]
struct PendingWrite {
  uid: i64,
  workspace_id: String,
  object_id: String,
  write_id: u64,
  payload_size: usize,
}

impl Eq for PendingWrite {}
impl PartialEq for PendingWrite {
  fn eq(&self, other: &Self) -> bool {
    self.write_id == other.write_id
  }
}

impl Ord for PendingWrite {
  fn cmp(&self, other: &Self) -> Ordering {
    self.write_id.cmp(&other.write_id).reverse()
  }
}

impl PartialOrd for PendingWrite {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}
