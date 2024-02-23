use crate::biz::collab::metrics::CollabMetrics;
use crate::biz::snapshot::cache::SnapshotCache;
use crate::biz::snapshot::queue::{PendingItem, PendingQueue};
use crate::state::RedisClient;
use app_error::AppError;
use async_stream::stream;
use database::collab::{create_snapshot_and_maintain_limit, COLLAB_SNAPSHOT_LIMIT};
use database_entity::dto::InsertSnapshotParams;
use futures_util::StreamExt;

use sqlx::PgPool;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::time::interval;
use tracing::{error, trace};
use validator::Validate;

pub type SnapshotCommandReceiver = tokio::sync::mpsc::Receiver<SnapshotCommand>;
pub type SnapshotCommandSender = tokio::sync::mpsc::Sender<SnapshotCommand>;

pub const SNAPSHOT_TICK_INTERVAL: Duration = Duration::from_secs(30);

pub enum SnapshotCommand {
  InsertSnapshot(InsertSnapshotParams),
  Tick(tokio::sync::oneshot::Sender<SnapshotMetric>),
}

#[derive(Clone)]
pub struct SnapshotControl {
  command_sender: SnapshotCommandSender,
}

impl SnapshotControl {
  pub async fn new(
    redis_client: RedisClient,
    pg_pool: PgPool,
    collab_metrics: Arc<CollabMetrics>,
  ) -> Self {
    let redis_client = Arc::new(Mutex::new(redis_client));
    let (command_sender, rx) = tokio::sync::mpsc::channel(1000);
    let cache = SnapshotCache::new(redis_client);

    // Load remaining items from cache
    let remaining_item = cache
      .keys(SNAPSHOT_PREFIX)
      .await
      .unwrap_or_default()
      .into_iter()
      .flat_map(|key| SnapshotKey(key).to_pending_item())
      .collect::<Vec<_>>();

    let runner = SnapshotCommandRunner::new(pg_pool, cache, rx, remaining_item);
    tokio::spawn(runner.run());

    let cloned_sender = command_sender.clone();
    tokio::spawn(async move {
      let mut interval = interval(SNAPSHOT_TICK_INTERVAL);
      loop {
        interval.tick().await;
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Err(err) = cloned_sender.send(SnapshotCommand::Tick(tx)).await {
          error!("Failed to send tick command: {}", err);
        }

        if let Ok(metric) = rx.await {
          collab_metrics.record_write_snapshot(
            metric.success_write_snapshot_count,
            metric.total_write_snapshot_count,
          );
        }
      }
    });

    Self { command_sender }
  }

  pub async fn queue_snapshot(&self, params: InsertSnapshotParams) -> Result<(), AppError> {
    params.validate()?;
    trace!("Queuing snapshot for {}", params.object_id);
    self
      .command_sender
      .send(SnapshotCommand::InsertSnapshot(params))
      .await
      .map_err(|err| AppError::Internal(err.into()))?;
    Ok(())
  }
}

struct SnapshotCommandRunner {
  pg_pool: PgPool,
  queue: RwLock<PendingQueue>,
  cache: SnapshotCache,
  recv: Option<SnapshotCommandReceiver>,
  success_attempts: AtomicU64,
  total_attempts: AtomicU64,
}
impl SnapshotCommandRunner {
  fn new(
    pg_pool: PgPool,
    cache: SnapshotCache,
    recv: SnapshotCommandReceiver,
    pending_item: Vec<PendingItem>,
  ) -> Self {
    let mut queue = PendingQueue::new();
    for item in pending_item {
      queue.push_item(item);
    }
    Self {
      pg_pool,
      queue: RwLock::new(queue),
      cache,
      recv: Some(recv),
      success_attempts: Default::default(),
      total_attempts: Default::default(),
    }
  }

  async fn run(mut self) {
    let mut receiver = self.recv.take().expect("Only take once");
    let stream = stream! {
      while let Some(cmd) = receiver.recv().await {
         yield cmd;
      }
    };

    stream
      .for_each(|command| async {
        self.handle_command(command).await;
      })
      .await;
  }

  async fn handle_command(&self, command: SnapshotCommand) {
    match command {
      SnapshotCommand::InsertSnapshot(params) => {
        let mut queue = self.queue.write().await;
        let item = queue.generate_item(
          params.workspace_id,
          params.object_id,
          params.encoded_collab_v1.len(),
        );
        let key = SnapshotKey::from_pending_item(&item);
        queue.push_item(item);
        drop(queue);

        if let Err(err) = self.cache.insert(&key.0, params.encoded_collab_v1).await {
          error!("Failed to insert snapshot to cache: {}", err);
        }
      },
      SnapshotCommand::Tick(tx) => {
        if let Err(e) = self.process_next_batch().await {
          error!("Failed to process next batch: {}", e);
        }

        let _ = tx.send(SnapshotMetric {
          success_write_snapshot_count: self.success_attempts.load(Ordering::Relaxed) as i64,
          total_write_snapshot_count: self.total_attempts.load(Ordering::Relaxed) as i64,
        });
      },
    }
  }

  async fn process_next_batch(&self) -> Result<(), AppError> {
    let next_item = match self.queue.write().await.pop() {
      Some(item) => item,
      None => return Ok(()), // No items to process
    };

    let key = SnapshotKey::from_pending_item(&next_item);
    let data = match self.cache.try_get(&key.0).await {
      Ok(data) => data,
      Err(_) => {
        if cfg!(debug_assertions) {
          error!("Failed to get snapshot from cache: {}", key.0);
        }
        self.queue.write().await.push_item(next_item);
        return Ok(());
      },
    };

    self.total_attempts.fetch_add(1, Ordering::Relaxed);
    let transaction = match self.pg_pool.try_begin().await {
      Ok(Some(tx)) => tx,
      _ => {
        self.queue.write().await.push_item(next_item);
        return Ok(());
      },
    };

    match create_snapshot_and_maintain_limit(
      transaction,
      &next_item.workspace_id,
      &next_item.object_id,
      &data,
      COLLAB_SNAPSHOT_LIMIT,
    )
    .await
    {
      Ok(_) => {
        trace!(
          "successfully created snapshot for {}, remaining task: {}",
          next_item.object_id,
          self.queue.read().await.len()
        );
        let _ = self.cache.remove(&key.0).await;
        self.success_attempts.fetch_add(1, Ordering::Relaxed);
        Ok(())
      },
      Err(e) => {
        self.queue.write().await.push_item(next_item);
        Err(e)
      },
    }
  }
}

const SNAPSHOT_PREFIX: &str = "full_snapshot";
struct SnapshotKey(String);

impl SnapshotKey {
  // Constructs a SnapshotKey from InsertSnapshotParams and a sequence number.

  fn from_pending_item(item: &PendingItem) -> Self {
    Self(format!(
      "{}:{}:{}:{}:{}",
      SNAPSHOT_PREFIX, item.workspace_id, item.object_id, item.seq, item.data_len
    ))
  }

  // Parses the SnapshotKey back into workspace_id, object_id, and seq.
  fn to_pending_item(&self) -> Option<PendingItem> {
    let parts: Vec<&str> = self.0.split(':').collect();
    if parts.len() == 5 {
      let workspace_id = parts[1].to_string();
      let object_id = parts[2].to_string();
      let seq = parts[3].parse::<i64>().unwrap_or(0);
      let data_len = parts[4].parse::<usize>().unwrap_or(0);

      Some(PendingItem {
        workspace_id,
        object_id,
        seq,
        data_len,
      })
    } else {
      None
    }
  }
}

pub struct SnapshotMetric {
  success_write_snapshot_count: i64,
  total_write_snapshot_count: i64,
}
