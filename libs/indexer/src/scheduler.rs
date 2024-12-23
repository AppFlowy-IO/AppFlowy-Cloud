use crate::collab_indexer::{Indexer, IndexerProvider};
use crate::entity::EmbeddingRecord;
use crate::error::IndexerError;
use crate::metrics::EmbeddingMetrics;
use crate::queue::add_background_embed_task;
use crate::thread_pool::{ThreadPoolNoAbort, ThreadPoolNoAbortBuilder};
use crate::vector::embedder::Embedder;
use crate::vector::open_ai;
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
use database::collab::CollabStorage;
use database::index::upsert_collab_embeddings;
use database::workspace::select_workspace_settings;
use database_entity::dto::{AFCollabEmbeddedChunk, CollabParams};
use infra::env_util::get_env_var;
use rayon::prelude::*;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::cmp::max;
use std::collections::HashSet;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::time::timeout;
use tracing::{debug, error, info, instrument, trace, warn};
use uuid::Uuid;

pub struct IndexerScheduler {
  pub(crate) indexer_provider: Arc<IndexerProvider>,
  pub(crate) pg_pool: PgPool,
  pub(crate) storage: Arc<dyn CollabStorage>,
  pub(crate) threads: Arc<ThreadPoolNoAbort>,
  #[allow(dead_code)]
  pub(crate) metrics: Arc<EmbeddingMetrics>,
  write_embedding_tx: UnboundedSender<EmbeddingRecord>,
  gen_embedding_tx: mpsc::Sender<PendingUnindexedCollab>,
  config: IndexerConfiguration,
  redis_client: ConnectionManager,
}

#[derive(Debug)]
pub struct IndexerConfiguration {
  pub enable: bool,
  pub openai_api_key: String,
  pub enable_background_indexing: bool,
  /// High watermark for the number of embeddings that can be buffered before being written to the database.
  pub embedding_buffer_size: usize,
}

impl IndexerScheduler {
  pub fn new(
    indexer_provider: Arc<IndexerProvider>,
    pg_pool: PgPool,
    storage: Arc<dyn CollabStorage>,
    metrics: Arc<EmbeddingMetrics>,
    mut config: IndexerConfiguration,
    redis_client: ConnectionManager,
  ) -> Arc<Self> {
    // Since threads often block while waiting for I/O, you can use more threads than CPU cores to improve concurrency.
    // A good rule of thumb is 2x to 10x the number of CPU cores
    let num_thread = max(
      get_env_var("APPFLOWY_INDEXER_SCHEDULER_NUM_THREAD", "50")
        .parse::<usize>()
        .unwrap_or(50),
      5,
    );

    if num_thread > config.embedding_buffer_size {
      warn!(
        "Number of threads {} is greater than embedding_buffer_size {}, set to {}",
        num_thread, config.embedding_buffer_size, num_thread
      );
      config.embedding_buffer_size = num_thread;
    }

    info!("Indexer scheduler config: {:?}", config);
    let (write_embedding_tx, write_embedding_rx) = unbounded_channel::<EmbeddingRecord>();
    let (gen_embedding_tx, gen_embedding_rx) =
      mpsc::channel::<PendingUnindexedCollab>(config.embedding_buffer_size);
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
      write_embedding_tx,
      gen_embedding_tx,
      config,
      redis_client,
    });

    info!(
      "Indexer scheduler is enabled: {}, num threads: {}",
      this.index_enabled(),
      num_thread
    );

    if this.index_enabled() {
      tokio::spawn(spawn_rayon_generate_embeddings(
        gen_embedding_rx,
        Arc::downgrade(&this),
        num_thread,
      ));

      tokio::spawn(spawn_pg_write_embeddings(
        write_embedding_rx,
        this.pg_pool.clone(),
        this.metrics.clone(),
      ));
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

  pub(crate) fn create_embedder(&self) -> Result<Embedder, AppError> {
    if self.config.openai_api_key.is_empty() {
      return Err(AppError::AIServiceUnavailable(
        "OpenAI API key is empty".to_string(),
      ));
    }

    Ok(Embedder::OpenAI(open_ai::Embedder::new(
      self.config.openai_api_key.clone(),
    )))
  }

  pub fn create_search_embeddings(
    &self,
    request: EmbeddingRequest,
  ) -> Result<OpenAIEmbeddingResponse, AppError> {
    let embedder = self.create_embedder()?;
    let embeddings = embedder.embed(request)?;
    Ok(embeddings)
  }

  pub fn embed_in_background(
    &self,
    pending_collab: PendingUnindexedCollab,
  ) -> Result<(), AppError> {
    if !self.index_enabled() {
      return Ok(());
    }

    if !self.config.enable_background_indexing {
      return Ok(());
    }

    let redis_client = self.redis_client.clone();
    tokio::spawn(add_background_embed_task(
      redis_client,
      vec![pending_collab.into()],
    ));
    Ok(())
  }

  pub fn embed_immediately(&self, pending_collab: PendingUnindexedCollab) -> Result<(), AppError> {
    if !self.index_enabled() {
      return Ok(());
    }
    if let Err(err) = self.gen_embedding_tx.try_send(pending_collab) {
      match err {
        TrySendError::Full(pending) => {
          warn!("[Embedding] Embedding queue is full, embedding in background");
          self.embed_in_background(pending)?;
        },
        TrySendError::Closed(_) => {
          error!("Failed to send embedding record: channel closed");
        },
      }
    }

    Ok(())
  }

  pub fn index_pending_collab_one(
    &self,
    pending_collab: PendingUnindexedCollab,
    _background: bool,
  ) -> Result<(), AppError> {
    if !self.index_enabled() {
      return Ok(());
    }

    let indexer = self
      .indexer_provider
      .indexer_for(&pending_collab.collab_type);
    if indexer.is_none() {
      return Ok(());
    }

    let _ = self.embed_immediately(pending_collab);
    Ok(())
  }

  pub fn index_pending_collabs(
    &self,
    mut pending_collabs: Vec<PendingUnindexedCollab>,
    _background: bool,
  ) -> Result<(), AppError> {
    if !self.index_enabled() {
      return Ok(());
    }

    pending_collabs.retain(|collab| self.is_indexing_enabled(&collab.collab_type));
    if pending_collabs.is_empty() {
      return Ok(());
    }

    info!("indexing {} collabs", pending_collabs.len());
    for pending_collab in pending_collabs {
      let _ = self.embed_immediately(pending_collab);
    }

    Ok(())
  }

  pub async fn index_collab_immediately(
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
    let embedder = self.create_embedder()?;

    let lock = collab.read().await;
    let chunks = indexer.create_embedded_chunks(&lock, embedder.model())?;
    drop(lock); // release the read lock ASAP

    if chunks.is_empty() {
      return Ok(());
    }

    self.embed_immediately(PendingUnindexedCollab {
      workspace_id: Uuid::parse_str(workspace_id)?,
      object_id: object_id.to_string(),
      collab_type: collab_type.clone(),
      data: UnindexedData::UnindexedChunks(chunks),
    })?;

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

async fn spawn_rayon_generate_embeddings(
  mut rx: mpsc::Receiver<PendingUnindexedCollab>,
  scheduler: Weak<IndexerScheduler>,
  buffer_size: usize,
) {
  let mut buf = Vec::with_capacity(buffer_size);
  loop {
    let n = rx.recv_many(&mut buf, buffer_size).await;
    let scheduler = match scheduler.upgrade() {
      Some(scheduler) => scheduler,
      None => {
        error!("[Embedding] Failed to upgrade scheduler");
        break;
      },
    };

    if n == 0 {
      info!("[Embedding] Stop generating embeddings");
      break;
    }

    let start = Instant::now();
    let records = buf.drain(..n).collect::<Vec<_>>();
    trace!(
      "[Embedding] received {} embeddings to generate",
      records.len()
    );
    let metrics = scheduler.metrics.clone();
    let threads = scheduler.threads.clone();
    let indexer_provider = scheduler.indexer_provider.clone();
    let write_embedding_tx = scheduler.write_embedding_tx.clone();
    let embedder = scheduler.create_embedder();
    let result = tokio::task::spawn_blocking(move || {
      match embedder {
        Ok(embedder) => {
          records.into_par_iter().for_each(|record| {
            let result = threads.install(|| {
              let indexer = indexer_provider.indexer_for(&record.collab_type);
              match process_collab(&embedder, indexer, &record.object_id, record.data, &metrics) {
                Ok(Some((tokens_used, contents))) => {
                  if let Err(err) = write_embedding_tx.send(EmbeddingRecord {
                    workspace_id: record.workspace_id,
                    object_id: record.object_id,
                    collab_type: record.collab_type,
                    tokens_used,
                    contents,
                  }) {
                    error!("Failed to send embedding record: {}", err);
                  }
                },
                Ok(None) => {
                  debug!("No embedding for collab:{}", record.object_id);
                },
                Err(err) => {
                  warn!(
                    "Failed to create embeddings content for collab:{}, error:{}",
                    record.object_id, err
                  );
                },
              }
            });

            if let Err(err) = result {
              error!("Failed to install a task to rayon thread pool: {}", err);
            }
          });
        },
        Err(err) => error!("[Embedding] Failed to create embedder: {}", err),
      }
      Ok::<_, IndexerError>(())
    })
    .await;

    match result {
      Ok(Ok(_)) => {
        scheduler
          .metrics
          .record_gen_embedding_time(n as u32, start.elapsed().as_millis());
        trace!("Successfully generated embeddings");
      },
      Ok(Err(err)) => error!("Failed to generate embeddings: {}", err),
      Err(err) => error!("Failed to spawn a task to generate embeddings: {}", err),
    }
  }
}

const EMBEDDING_RECORD_BUFFER_SIZE: usize = 10;
async fn spawn_pg_write_embeddings(
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

    trace!("[Embedding] received {} embeddings to write", n);
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

#[instrument(level = "trace", skip_all)]
pub(crate) async fn batch_insert_records(
  pg_pool: &PgPool,
  records: Vec<EmbeddingRecord>,
) -> Result<(), AppError> {
  let mut seen = HashSet::new();
  let records = records
    .into_iter()
    .filter(|record| seen.insert(record.object_id.clone()))
    .collect::<Vec<_>>();

  let mut txn = pg_pool.begin().await?;
  for record in records {
    upsert_collab_embeddings(
      &mut txn,
      &record.workspace_id,
      &record.object_id,
      record.collab_type,
      record.tokens_used,
      record.contents,
    )
    .await?;
  }
  txn.commit().await.map_err(|e| {
    error!("[Embedding] Failed to commit transaction: {:?}", e);
    e
  })?;
  Ok(())
}

/// This function must be called within the rayon thread pool.
fn process_collab(
  embdder: &Embedder,
  indexer: Option<Arc<dyn Indexer>>,
  object_id: &str,
  data: UnindexedData,
  metrics: &EmbeddingMetrics,
) -> Result<Option<(u32, Vec<AFCollabEmbeddedChunk>)>, AppError> {
  if let Some(indexer) = indexer {
    metrics.record_embed_count(1);

    let start_time = Instant::now();
    let chunks = match data {
      UnindexedData::UnindexedEncodeCollab(encoded_collab) => {
        let encode_collab = EncodedCollab::decode_from_bytes(&encoded_collab)
          .map_err(|err| AppError::Internal(anyhow!("Failed to decode encoded collab: {}", err)))?;
        let collab = Collab::new_with_source(
          CollabOrigin::Empty,
          object_id,
          DataSource::DocStateV1(encode_collab.doc_state.into()),
          vec![],
          false,
        )
        .map_err(|err| AppError::Internal(err.into()))?;
        indexer.create_embedded_chunks(&collab, embdder.model())?
      },
      UnindexedData::UnindexedChunks(chunks) => chunks,
    };

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

pub struct PendingUnindexedCollab {
  pub workspace_id: Uuid,
  pub object_id: String,
  pub collab_type: CollabType,
  pub data: UnindexedData,
}

pub enum UnindexedData {
  UnindexedEncodeCollab(Bytes),
  UnindexedChunks(Vec<AFCollabEmbeddedChunk>),
}

impl From<(Uuid, &CollabParams)> for PendingUnindexedCollab {
  fn from((workspace_id, params): (Uuid, &CollabParams)) -> Self {
    Self {
      workspace_id,
      object_id: params.object_id.clone(),
      collab_type: params.collab_type.clone(),
      data: UnindexedData::UnindexedEncodeCollab(params.encoded_collab_v1.clone()),
    }
  }
}
