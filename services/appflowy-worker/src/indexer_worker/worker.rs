use app_error::AppError;
use indexer::collab_indexer::{Indexer, IndexerProvider};
use indexer::entity::EmbeddingRecord;
use indexer::metrics::EmbeddingMetrics;
use indexer::queue::{
  ack_task, default_indexer_group_option, ensure_indexer_consumer_group,
  read_background_embed_tasks,
};
use indexer::scheduler::{spawn_pg_write_embeddings, UnindexedCollabTask, UnindexedData};
use indexer::thread_pool::ThreadPoolNoAbort;
use indexer::vector::embedder::Embedder;
use indexer::vector::open_ai;
use rayon::prelude::*;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::RwLock;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{error, info, trace};

#[derive(Debug)]
pub struct BackgroundIndexerConfig {
  pub open_api_key: String,
  pub tick_interval_secs: u64,
}

pub async fn run_background_indexer(
  pg_pool: PgPool,
  mut redis_client: ConnectionManager,
  embed_metrics: Arc<EmbeddingMetrics>,
  threads: Arc<ThreadPoolNoAbort>,
  config: BackgroundIndexerConfig,
) {
  if config.open_api_key.is_empty() {
    error!("OpenAI API key is not set. Stop background indexer");
    return;
  }

  let indexer_provider = IndexerProvider::new();
  info!("Starting background indexer...");
  if let Err(err) = ensure_indexer_consumer_group(&mut redis_client).await {
    error!("Failed to ensure indexer consumer group: {:?}", err);
  }

  let latest_write_embedding_err = Arc::new(RwLock::new(None));
  let (write_embedding_tx, write_embedding_rx) = unbounded_channel::<EmbeddingRecord>();
  let write_embedding_task_fut = spawn_pg_write_embeddings(
    write_embedding_rx,
    pg_pool,
    embed_metrics.clone(),
    latest_write_embedding_err.clone(),
  );

  let process_tasks_task_fut = process_upcoming_tasks(
    &mut redis_client,
    embed_metrics,
    indexer_provider,
    threads,
    config,
    write_embedding_tx,
    latest_write_embedding_err,
  );

  tokio::select! {
    _ = write_embedding_task_fut => {
      error!("[Background Embedding] Write embedding task stopped");
    },
    _ = process_tasks_task_fut => {
      error!("[Background Embedding] Process tasks task stopped");
    },
  }
}

async fn process_upcoming_tasks(
  redis_client: &mut ConnectionManager,
  metrics: Arc<EmbeddingMetrics>,
  indexer_provider: Arc<IndexerProvider>,
  threads: Arc<ThreadPoolNoAbort>,
  config: BackgroundIndexerConfig,
  sender: UnboundedSender<EmbeddingRecord>,
  latest_write_embedding_err: Arc<RwLock<Option<AppError>>>,
) {
  let options = default_indexer_group_option(threads.current_num_threads());
  let mut interval = interval(Duration::from_secs(config.tick_interval_secs));
  interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
  interval.tick().await;

  loop {
    interval.tick().await;

    let latest_error = latest_write_embedding_err.write().await.take();
    if let Some(err) = latest_error {
      if matches!(err, AppError::ActionTimeout(_)) {
        info!(
          "[Background Embedding] last write embedding task failed with timeout, waiting for 30s before retrying..."
        );
        tokio::time::sleep(Duration::from_secs(30)).await;
      }
    }

    if let Ok(replay) = read_background_embed_tasks(redis_client, &options).await {
      let all_keys: Vec<String> = replay
        .keys
        .iter()
        .flat_map(|key| key.ids.iter().map(|stream_id| stream_id.id.clone()))
        .collect();

      for key in replay.keys {
        info!("[Background Embedding] processing {} tasks", key.ids.len());
        key.ids.into_par_iter().for_each(|stream_id| {
          let result = threads.install(|| match UnindexedCollabTask::try_from(&stream_id) {
            Ok(task) => {
              if let Some(indexer) = indexer_provider.indexer_for(&task.collab_type) {
                let start = Instant::now();
                let embedder = create_embedder(&config);
                let result = handle_task(embedder, indexer, task);
                let cost = start.elapsed().as_millis();
                metrics.record_write_embedding_time(cost);
                if let Some(record) = result {
                  let _ = sender.send(record);
                }
              }
            },
            Err(err) => {
              error!(
                "[Background Embedding] failed to parse embedder task: {:?}",
                err
              );
            },
          });
          if let Err(err) = result {
            error!(
              "[Background Embedding] Failed to process embedder task: {:?}",
              err
            );
          }
        });
      }

      if !all_keys.is_empty() {
        match ack_task(redis_client, all_keys, true).await {
          Ok(_) => trace!("[Background embedding]: delete tasks from stream"),
          Err(err) => {
            error!("[Background Embedding] Failed to ack tasks: {:?}", err);
          },
        }
      }
    }
  }
}

fn handle_task(
  embedder: Embedder,
  indexer: Arc<dyn Indexer>,
  task: UnindexedCollabTask,
) -> Option<EmbeddingRecord> {
  trace!(
    "[Background Embedding] processing task: {}, content:{:?}, collab_type: {}",
    task.object_id,
    task.data,
    task.collab_type
  );
  let chunks = match task.data {
    UnindexedData::UnindexedText(text) => indexer
      .create_embedded_chunks_from_text(task.object_id.clone(), text, embedder.model())
      .ok()?,
  };
  let embeddings = indexer.embed(&embedder, chunks).ok()?;
  embeddings.map(|embeddings| EmbeddingRecord {
    workspace_id: task.workspace_id,
    object_id: task.object_id,
    collab_type: task.collab_type,
    tokens_used: embeddings.tokens_consumed,
    contents: embeddings.params,
  })
}

fn create_embedder(config: &BackgroundIndexerConfig) -> Embedder {
  Embedder::OpenAI(open_ai::Embedder::new(config.open_api_key.clone()))
}
