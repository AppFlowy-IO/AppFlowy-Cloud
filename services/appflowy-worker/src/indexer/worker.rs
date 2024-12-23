use indexer::metrics::EmbeddingMetrics;
use indexer::queue::{
  default_indexer_group_option, ensure_indexer_consumer_group, read_background_embed_tasks,
  EmbedderTask,
};
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{error, info};

pub async fn run_background_indexer(
  pg_pool: PgPool,
  mut redis_client: ConnectionManager,
  metrics: Arc<EmbeddingMetrics>,
  tick_interval_secs: u64,
) {
  info!("Starting background indexer...");
  if let Err(err) = ensure_indexer_consumer_group(&mut redis_client).await {
    error!("Failed to ensure indexer consumer group: {:?}", err);
  }
}

async fn process_upcoming_tasks(
  redis_client: &mut ConnectionManager,
  pg_pool: PgPool,
  metrics: Arc<EmbeddingMetrics>,
  tick_interval_secs: u64,
) {
  let options = default_indexer_group_option(10);
  let mut interval = interval(Duration::from_secs(tick_interval_secs));
  interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
  interval.tick().await;

  loop {
    interval.tick().await;
    if let Ok(replay) = read_background_embed_tasks(redis_client, &options).await {
      for key in replay.keys {
        for stream_id in key.ids {
          match EmbedderTask::try_from(&stream_id) {
            Ok(task) => {
              //
            },
            Err(err) => {
              error!("Failed to parse embedder task: {:?}", err);
            },
          }
        }
      }
    }
  }
}

async fn handle_task(pg_pool: PgPool, task: EmbedderTask) {}
