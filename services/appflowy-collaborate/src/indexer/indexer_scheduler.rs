use crate::config::get_env_var;
use crate::indexer::metrics::EmbeddingMetrics;
use crate::indexer::vector::embedder::Embedder;
use crate::indexer::vector::open_ai;
use crate::indexer::{Indexer, IndexerProvider};
use crate::thread_pool_no_abort::{ThreadPoolNoAbort, ThreadPoolNoAbortBuilder};
use anyhow::anyhow;
use app_error::AppError;
use appflowy_ai_client::dto::{EmbeddingRequest, OpenAIEmbeddingResponse};
use bytes::Bytes;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::lock::RwLock;
use collab::preclude::Collab;
use collab_entity::CollabType;
use dashmap::DashMap;
use database::collab::{CollabStorage, GetCollabOrigin};
use database::index::{get_collabs_without_embeddings, upsert_collab_embeddings};
use database::workspace::select_workspace_settings;
use database_entity::dto::{AFCollabEmbeddedChunk, CollabParams};
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use rayon::prelude::*;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::time::timeout;
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

pub struct IndexerScheduler {
  indexer_provider: Arc<IndexerProvider>,
  pg_pool: PgPool,
  #[allow(dead_code)]
  storage: Arc<dyn CollabStorage>,
  threads: Arc<ThreadPoolNoAbort>,
  #[allow(dead_code)]
  metrics: Arc<EmbeddingMetrics>,
  schedule_tx: UnboundedSender<EmbeddingRecord>,
  config: IndexerConfiguration,
  active_tasks: Arc<DashMap<String, ActiveTask>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveTask {
  object_id: String,
  created_at: i64,
}

impl ActiveTask {
  fn new(object_id: String) -> Self {
    Self {
      object_id,
      created_at: chrono::Utc::now().timestamp(),
    }
  }
}

pub struct IndexerConfiguration {
  pub enable: bool,
  pub openai_api_key: String,
}

impl IndexerScheduler {
  pub fn new(
    indexer_provider: Arc<IndexerProvider>,
    pg_pool: PgPool,
    storage: Arc<dyn CollabStorage>,
    metrics: Arc<EmbeddingMetrics>,
    config: IndexerConfiguration,
  ) -> Arc<Self> {
    let (schedule_tx, rx) = unbounded_channel::<EmbeddingRecord>();
    // Since threads often block while waiting for I/O, you can use more threads than CPU cores to improve concurrency.
    // A good rule of thumb is 2x to 10x the number of CPU cores
    let num_thread = get_env_var("APPFLOWY_INDEXER_SCHEDULER_NUM_THREAD", "10")
      .parse::<usize>()
      .unwrap_or(10);
    let threads = Arc::new(
      ThreadPoolNoAbortBuilder::new()
        .num_threads(num_thread)
        .thread_name(|index| format!("embedding-request-{index}"))
        .build()
        .unwrap(),
    );

    let this = Arc::new(Self {
      indexer_provider,
      pg_pool,
      storage,
      threads,
      metrics,
      schedule_tx,
      config,
      active_tasks: Arc::new(Default::default()),
    });

    info!(
      "Indexer scheduler is enabled: {}, num threads: {}",
      this.index_enabled(),
      num_thread
    );

    if this.index_enabled() {
      tokio::spawn(spawn_write_indexing(
        rx,
        this.pg_pool.clone(),
        this.metrics.clone(),
      ));
      // tokio::spawn(handle_unindexed_collabs(this.clone()));
    }

    this
  }

  fn index_enabled(&self) -> bool {
    // if indexing is disabled, return false
    if !self.config.enable {
      return false;
    }

    // if openai api key is empty, return false
    if self.config.openai_api_key.is_empty() {
      return false;
    }

    true
  }

  pub fn is_indexing_enabled(&self, collab_type: &CollabType) -> bool {
    self.indexer_provider.is_indexing_enabled(collab_type)
  }

  fn create_embedder(&self) -> Result<Embedder, AppError> {
    if self.config.openai_api_key.is_empty() {
      return Err(AppError::AIServiceUnavailable(
        "OpenAI API key is empty".to_string(),
      ));
    }

    Ok(Embedder::OpenAI(open_ai::Embedder::new(
      self.config.openai_api_key.clone(),
    )))
  }

  pub fn embeddings(&self, request: EmbeddingRequest) -> Result<OpenAIEmbeddingResponse, AppError> {
    let embedder = self.create_embedder()?;
    let embeddings = embedder.embed(request)?;
    Ok(embeddings)
  }

  pub fn index_encoded_collab_one<T>(
    &self,
    workspace_id: &str,
    indexed_collab: T,
  ) -> Result<(), AppError>
  where
    T: Into<IndexedCollab>,
  {
    if !self.index_enabled() {
      return Ok(());
    }

    let indexed_collab = indexed_collab.into();
    let indexer = self
      .indexer_provider
      .indexer_for(&indexed_collab.collab_type);
    if indexer.is_none() {
      return Ok(());
    }

    let embedder = self.create_embedder()?;
    let workspace_id = Uuid::parse_str(workspace_id)?;
    let tx = self.schedule_tx.clone();

    let metrics = self.metrics.clone();
    let active_task = self.active_tasks.clone();
    let task = ActiveTask::new(indexed_collab.object_id.clone());
    let task_created_at = task.created_at;
    active_task.insert(indexed_collab.object_id.clone(), task);
    let threads = self.threads.clone();

    rayon::spawn(move || {
      let result = threads.install(|| {
        if !should_embed(&active_task, &indexed_collab.object_id, task_created_at) {
          return;
        }

        match process_collab(&embedder, indexer, &indexed_collab, &metrics) {
          Ok(Some((tokens_used, contents))) => {
            if let Err(err) = tx.send(EmbeddingRecord {
              workspace_id,
              object_id: indexed_collab.object_id,
              tokens_used,
              contents,
            }) {
              error!("Failed to send embedding record: {}", err);
            }
          },
          Ok(None) => debug!("No embedding for collab:{}", indexed_collab.object_id),
          Err(err) => {
            warn!(
              "Failed to create embeddings content for collab:{}, error:{}",
              indexed_collab.object_id, err
            );
          },
        }
      });

      if let Err(err) = result {
        error!("Failed to spawn a task to index collab: {}", err);
      }
    });
    Ok(())
  }

  pub fn index_encoded_collabs(
    &self,
    workspace_id: &str,
    mut indexed_collabs: Vec<IndexedCollab>,
  ) -> Result<(), AppError> {
    if !self.index_enabled() {
      return Ok(());
    }

    indexed_collabs.retain(|collab| self.is_indexing_enabled(&collab.collab_type));
    if indexed_collabs.is_empty() {
      return Ok(());
    }

    info!("indexing {} collabs", indexed_collabs.len());
    let embedder = self.create_embedder()?;
    let workspace_id = Uuid::parse_str(workspace_id)?;
    let indexer_provider = self.indexer_provider.clone();
    let threads = self.threads.clone();
    let tx = self.schedule_tx.clone();
    let metrics = self.metrics.clone();
    let active_task = self.active_tasks.clone();
    rayon::spawn(move || {
      let embeddings_list = indexed_collabs
        .into_par_iter()
        .filter_map(|collab| {
          let indexer = indexer_provider.indexer_for(&collab.collab_type)?;
          let task = ActiveTask::new(collab.object_id.clone());
          let task_created_at = task.created_at;
          active_task.insert(collab.object_id.clone(), task);
          threads
            .install(|| {
              if !should_embed(&active_task, &collab.object_id, task_created_at) {
                return None;
              }
              process_collab(&embedder, Some(indexer), &collab, &metrics).ok()
            })
            .ok()
        })
        .filter_map(|result| result)
        .filter_map(|result| result.map(|r| (r.0, r.1)))
        .collect::<Vec<_>>();

      for (tokens_used, contents) in embeddings_list {
        if contents.is_empty() {
          continue;
        }
        let object_id = contents[0].object_id.clone();
        if let Err(err) = tx.send(EmbeddingRecord {
          workspace_id,
          object_id,
          tokens_used,
          contents,
        }) {
          error!("Failed to send embedding record: {}", err);
        }
      }
    });

    Ok(())
  }

  pub async fn index_collab(
    &self,
    workspace_id: &str,
    object_id: &str,
    collab: &Arc<RwLock<Collab>>,
    collab_type: &CollabType,
  ) -> Result<(), AppError> {
    if !self.index_enabled() {
      return Ok(());
    }

    if !self.is_indexing_enabled(collab_type) {
      return Ok(());
    }

    let indexer = self
      .indexer_provider
      .indexer_for(collab_type)
      .ok_or_else(|| {
        AppError::Internal(anyhow!(
          "No indexer found for collab type {:?}",
          collab_type
        ))
      })?;
    let workspace_id = Uuid::parse_str(workspace_id)?;
    let embedder = self.create_embedder()?;

    let lock = collab.read().await;
    let chunks = indexer.create_embedded_chunks(&lock, embedder.model())?;
    drop(lock); // release the read lock ASAP

    let threads = self.threads.clone();
    let tx = self.schedule_tx.clone();
    let object_id = object_id.to_string();
    let metrics = self.metrics.clone();
    let active_tasks = self.active_tasks.clone();
    let task = ActiveTask::new(object_id.clone());
    let task_created_at = task.created_at;
    active_tasks.insert(object_id.clone(), task);

    rayon::spawn(move || {
      let start = Instant::now();
      metrics.record_embed_count(1);
      let result = threads.install(|| {
        if !should_embed(&active_tasks, &object_id, task_created_at) {
          return Ok(None);
        }
        indexer.embed(&embedder, chunks)
      });
      let duration = start.elapsed();
      metrics.record_generate_embedding_time(duration.as_millis());

      match result {
        Ok(embed_result) => match embed_result {
          Ok(Some(data)) => {
            if let Err(err) = tx.send(EmbeddingRecord {
              workspace_id,
              object_id: object_id.to_string(),
              tokens_used: data.tokens_consumed,
              contents: data.params,
            }) {
              error!("Failed to send embedding record: {}", err);
            }
          },
          Ok(None) => debug!("No embedding for collab:{}", object_id),
          Err(err) => {
            metrics.record_failed_embed_count(1);
            error!(
              "Failed to create embeddings content for collab:{}, error:{}",
              object_id, err
            );
          },
        },
        Err(err) => {
          error!("Failed to spawn a task to index collab: {}", err);
        },
      }
    });

    Ok(())
  }

  pub async fn can_index_workspace(&self, workspace_id: &str) -> Result<bool, AppError> {
    if !self.index_enabled() {
      return Ok(false);
    }

    let uuid = Uuid::parse_str(workspace_id)?;
    let settings = select_workspace_settings(&self.pg_pool, &uuid).await?;
    match settings {
      None => Ok(true),
      Some(settings) => Ok(!settings.disable_search_indexing),
    }
  }
}

/// Determines whether an object (Collab) should be processed for embedding.
///
/// it ensures that duplicate or unnecessary indexing tasks are avoided
/// by checking if the object is already in the active task list. If the object is
/// already being indexed, it prevents re-processing the same object. The function
/// compares the current task's timestamp with any existing active task for the same object
/// to ensure tasks are processed in order and without overlap.
#[inline]
fn should_embed(
  active_tasks: &DashMap<String, ActiveTask>,
  object_id: &str,
  created_at: i64,
) -> bool {
  let should_embed = active_tasks
    .get(object_id)
    .map(|t| t.created_at)
    .unwrap_or(0)
    >= created_at;
  if !should_embed {
    trace!("[Embedding] Skipping embedding for object: {} because a newer task is already in progress. Previous task with the same object ID has been overridden.", object_id);
  }
  should_embed
}

#[allow(dead_code)]
async fn handle_unindexed_collabs(scheduler: Arc<IndexerScheduler>) {
  // wait for 30 seconds before starting indexing
  tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

  let mut i = 0;
  let mut stream = get_unindexed_collabs(&scheduler.pg_pool, scheduler.storage.clone()).await;
  let record_tx = scheduler.schedule_tx.clone();
  let start = Instant::now();
  while let Some(result) = stream.next().await {
    if let Ok(embedder) = scheduler.create_embedder() {
      match result {
        Ok(collab) => {
          let workspace = collab.workspace_id;
          let oid = collab.object_id.clone();
          if let Err(err) = index_unindexd_collab(
            embedder,
            &scheduler.indexer_provider,
            scheduler.threads.clone(),
            collab,
            record_tx.clone(),
          )
          .await
          {
            // only logging error in debug mode. Will be enabled in production if needed.
            if cfg!(debug_assertions) {
              warn!("failed to index collab {}/{}: {}", workspace, oid, err);
            }
          } else {
            i += 1;
          }
        },
        Err(err) => {
          error!("failed to get unindexed document: {}", err);
        },
      }
    }
  }
  info!(
    "indexed {} unindexed collabs in {:?} after restart",
    i,
    start.elapsed()
  )
}

pub async fn get_unindexed_collabs(
  pg_pool: &PgPool,
  storage: Arc<dyn CollabStorage>,
) -> BoxStream<Result<UnindexedCollab, anyhow::Error>> {
  let cloned_storage = storage.clone();
  get_collabs_without_embeddings(pg_pool)
    .map(move |result| {
      let storage = cloned_storage.clone();
      async move {
        match result {
          Ok(cid) => match cid.collab_type {
            CollabType::Document => {
              let collab = storage
                .get_encode_collab(GetCollabOrigin::Server, cid.clone().into(), false)
                .await?;

              Ok(Some(UnindexedCollab {
                workspace_id: cid.workspace_id,
                object_id: cid.object_id,
                collab_type: cid.collab_type,
                collab,
              }))
            },
            _ => Ok::<_, anyhow::Error>(None),
          },
          Err(e) => Err(e.into()),
        }
      }
    })
    .filter_map(|future| async {
      match future.await {
        Ok(Some(unindexed_collab)) => Some(Ok(unindexed_collab)),
        Ok(None) => None,
        Err(e) => Some(Err(e)),
      }
    })
    .boxed()
}

#[allow(dead_code)]
async fn index_unindexd_collab(
  embedder: Embedder,
  indexer_provider: &Arc<IndexerProvider>,
  threads: Arc<ThreadPoolNoAbort>,
  unindexed: UnindexedCollab,
  record_tx: UnboundedSender<EmbeddingRecord>,
) -> Result<(), AppError> {
  if let Some(indexer) = indexer_provider.indexer_for(&unindexed.collab_type) {
    let object_id = unindexed.object_id.clone();
    let workspace_id = unindexed.workspace_id;

    rayon::spawn(move || {
      if let Ok(collab) = Collab::new_with_source(
        CollabOrigin::Empty,
        &unindexed.object_id,
        DataSource::DocStateV1(unindexed.collab.doc_state.into()),
        vec![],
        false,
      ) {
        if let Ok(chunks) = indexer.create_embedded_chunks(&collab, embedder.model()) {
          let result = threads.install(|| {
            if let Ok(Some(embeddings)) = indexer.embed(&embedder, chunks) {
              if let Err(err) = record_tx.send(EmbeddingRecord {
                workspace_id,
                object_id: object_id.clone(),
                tokens_used: embeddings.tokens_consumed,
                contents: embeddings.params,
              }) {
                error!("Failed to send embedding record: {}", err);
              }
            }
          });

          if let Err(err) = result {
            error!("Failed to spawn a task to index collab: {}", err);
          }
        }
      }
    });
  }
  Ok(())
}

const EMBEDDING_RECORD_BUFFER_SIZE: usize = 5;
async fn spawn_write_indexing(
  mut rx: UnboundedReceiver<EmbeddingRecord>,
  pg_pool: PgPool,
  metrics: Arc<EmbeddingMetrics>,
) {
  let mut buf = Vec::with_capacity(EMBEDDING_RECORD_BUFFER_SIZE);
  loop {
    let n = rx.recv_many(&mut buf, EMBEDDING_RECORD_BUFFER_SIZE).await;
    if n == 0 {
      info!("Stop writing embeddings");
      break;
    }

    let start = Instant::now();
    let records = buf.drain(..n).collect::<Vec<_>>();
    for record in records.iter() {
      info!(
        "[Embedding] generate collab:{} embeddings, tokens used: {}",
        record.object_id, record.tokens_used
      );
    }

    let result = timeout(
      Duration::from_secs(20),
      batch_insert_records(&pg_pool, records),
    )
    .await
    .unwrap_or_else(|_| {
      Err(AppError::Internal(anyhow!(
        "timeout when writing embeddings"
      )))
    });

    metrics.record_write_embedding_time(start.elapsed().as_millis());
    match result {
      Ok(_) => trace!("[Embedding] save {} embeddings to disk", n),
      Err(err) => error!("Failed to write collab embedding to disk:{}", err),
    }
  }
}

async fn batch_insert_records(
  pg_pool: &PgPool,
  records: Vec<EmbeddingRecord>,
) -> Result<(), AppError> {
  // deduplicate records
  let records = records
    .into_iter()
    .fold(Vec::<EmbeddingRecord>::new(), |mut acc, record| {
      if !acc.iter().any(|r| r.object_id == record.object_id) {
        acc.push(record);
      }
      acc
    });
  let mut txn = pg_pool.begin().await?;
  for record in records {
    upsert_collab_embeddings(
      &mut txn,
      &record.workspace_id,
      record.tokens_used,
      record.contents,
    )
    .await?;
  }
  txn.commit().await?;
  Ok(())
}

/// This function must be called within the rayon thread pool.
fn process_collab(
  embdder: &Embedder,
  indexer: Option<Arc<dyn Indexer>>,
  indexed_collab: &IndexedCollab,
  metrics: &EmbeddingMetrics,
) -> Result<Option<(u32, Vec<AFCollabEmbeddedChunk>)>, AppError> {
  if let Some(indexer) = indexer {
    metrics.record_embed_count(1);
    let encode_collab = EncodedCollab::decode_from_bytes(&indexed_collab.encoded_collab)?;
    let collab = Collab::new_with_source(
      CollabOrigin::Empty,
      &indexed_collab.object_id,
      DataSource::DocStateV1(encode_collab.doc_state.into()),
      vec![],
      false,
    )
    .map_err(|err| AppError::Internal(err.into()))?;

    let start_time = Instant::now();
    let chunks = indexer.create_embedded_chunks(&collab, embdder.model())?;
    let result = indexer.embed(embdder, chunks);
    let duration = start_time.elapsed();
    metrics.record_generate_embedding_time(duration.as_millis());

    match result {
      Ok(Some(embeddings)) => Ok(Some((embeddings.tokens_consumed, embeddings.params))),
      Ok(None) => Ok(None),
      Err(err) => {
        metrics.record_failed_embed_count(1);
        Err(err)
      },
    }
  } else {
    Ok(None)
  }
}

pub struct UnindexedCollab {
  pub workspace_id: Uuid,
  pub object_id: String,
  pub collab_type: CollabType,
  pub collab: EncodedCollab,
}

pub struct IndexedCollab {
  pub object_id: String,
  pub collab_type: CollabType,
  pub encoded_collab: Bytes,
}

struct EmbeddingRecord {
  workspace_id: Uuid,
  object_id: String,
  tokens_used: u32,
  contents: Vec<AFCollabEmbeddedChunk>,
}

impl From<&CollabParams> for IndexedCollab {
  fn from(params: &CollabParams) -> Self {
    Self {
      object_id: params.object_id.clone(),
      collab_type: params.collab_type.clone(),
      encoded_collab: params.encoded_collab_v1.clone(),
    }
  }
}
