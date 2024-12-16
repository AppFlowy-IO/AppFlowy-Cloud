use crate::config::get_env_var;
use crate::indexer::metrics::EmbeddingMetrics;
use crate::indexer::vector::embedder::Embedder;
use crate::indexer::vector::open_ai;
use crate::indexer::IndexerProvider;
use crate::thread_pool_no_abort::{ThreadPoolNoAbort, ThreadPoolNoAbortBuilder};
use actix::dev::Stream;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_ai_client::dto::{EmbeddingRequest, OpenAIEmbeddingResponse};
use async_stream::try_stream;
use bytes::Bytes;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::lock::RwLock;
use collab::preclude::Collab;
use collab_entity::CollabType;
use database::collab::{CollabStorage, GetCollabOrigin};
use database::index::{get_collabs_without_embeddings, upsert_collab_embeddings};
use database::workspace::select_workspace_settings;
use database_entity::dto::{AFCollabEmbeddedChunk, CollabParams};
use futures_util::StreamExt;
use rayon::prelude::*;
use sqlx::PgPool;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tracing::{error, info, trace, warn};
use uuid::Uuid;

pub struct IndexerScheduler {
  indexer_provider: Arc<IndexerProvider>,
  pg_pool: PgPool,
  storage: Arc<dyn CollabStorage>,
  threads: Arc<ThreadPoolNoAbort>,
  #[allow(dead_code)]
  metrics: Arc<EmbeddingMetrics>,
  schedule_tx: UnboundedSender<EmbeddingRecord>,
  config: IndexerConfiguration,
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
    });

    info!("Indexer scheduler is enabled: {}", this.index_enabled());
    if this.index_enabled() {
      tokio::spawn(spawn_write_indexing(rx, this.pg_pool.clone()));
      tokio::spawn(handle_unindexed_collabs(this.clone()));
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

    let embedder = self.create_embedder()?;
    let indexed_collab = indexed_collab.into();
    let workspace_id = Uuid::parse_str(workspace_id)?;
    let indexer_provider = self.indexer_provider.clone();
    let tx = self.schedule_tx.clone();

    let metrics = self.metrics.clone();
    rayon::spawn(move || {
      match process_collab(&embedder, &indexer_provider, &indexed_collab, &metrics) {
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
        Ok(None) => trace!("No embedding for collab:{}", indexed_collab.object_id),
        Err(err) => {
          warn!(
            "Failed to create embeddings content for collab:{}, error:{}",
            indexed_collab.object_id, err
          );
        },
      }
    });
    Ok(())
  }

  pub fn index_encoded_collabs(
    &self,
    workspace_id: &str,
    indexed_collabs: Vec<IndexedCollab>,
  ) -> Result<(), AppError> {
    if !self.index_enabled() {
      return Ok(());
    }

    let embedder = self.create_embedder()?;
    let workspace_id = Uuid::parse_str(workspace_id)?;
    let indexer_provider = self.indexer_provider.clone();
    let threads = self.threads.clone();
    let tx = self.schedule_tx.clone();
    let metrics = self.metrics.clone();
    rayon::spawn(move || {
      let results = threads.install(|| {
        indexed_collabs
          .into_par_iter()
          .filter_map(|collab| process_collab(&embedder, &indexer_provider, &collab, &metrics).ok())
          .filter_map(|result| result.map(|r| (r.0, r.1)))
          .collect::<Vec<_>>()
      });

      match results {
        Ok(embeddings_list) => {
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
        },
        Err(err) => {
          error!("Failed to process batch indexing: {}", err);
        },
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

    let workspace_id = Uuid::parse_str(workspace_id)?;
    let embedder = self.create_embedder()?;
    let indexer = self
      .indexer_provider
      .indexer_for(collab_type)
      .ok_or_else(|| {
        AppError::Internal(anyhow!(
          "No indexer found for collab type {:?}",
          collab_type
        ))
      })?;

    let lock = collab.read().await;
    let chunks = indexer.create_embedded_chunks(&lock, embedder.model())?;
    drop(lock); // release the read lock ASAP

    let threads = self.threads.clone();
    let tx = self.schedule_tx.clone();
    let object_id = object_id.to_string();
    let metrics = self.metrics.clone();
    rayon::spawn(move || {
      let start = Instant::now();
      metrics.record_embed_count(1);

      let result = indexer.embed_in_thread_pool(&embedder, chunks, &threads);
      let duration = start.elapsed();
      metrics.record_processing_time(duration.as_millis());

      match result {
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
        Ok(None) => warn!("No embedding for collab:{}", object_id),
        Err(err) => {
          metrics.record_failed_embed_count(1);
          error!(
            "Failed to create embeddings content for collab:{}, error:{}",
            object_id, err
          );
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

async fn handle_unindexed_collabs(scheduler: Arc<IndexerScheduler>) {
  // wait for 30 seconds before starting indexing
  tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

  let mut i = 0;
  let mut stream = get_unindexed_collabs(&scheduler.pg_pool, scheduler.storage.clone());
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

fn get_unindexed_collabs(
  pg_pool: &PgPool,
  storage: Arc<dyn CollabStorage>,
) -> Pin<Box<dyn Stream<Item = Result<UnindexedCollab, anyhow::Error>> + Send>> {
  let db = pg_pool.clone();
  Box::pin(try_stream! {
    let collabs = get_collabs_without_embeddings(&db).await?;
    if !collabs.is_empty() {
      info!("found {} unindexed collabs", collabs.len());
    }
    for cid in collabs {
      match &cid.collab_type {
        CollabType::Document => {
          let collab = storage
            .get_encode_collab(GetCollabOrigin::Server, cid.clone().into(), false)
            .await?;

          yield UnindexedCollab {
            workspace_id: cid.workspace_id,
            object_id: cid.object_id,
            collab_type: cid.collab_type,
            collab,
          };
        },
        CollabType::Database
        | CollabType::WorkspaceDatabase
        | CollabType::Folder
        | CollabType::DatabaseRow
        | CollabType::UserAwareness
        | CollabType::Unknown => { /* atm. only document types are supported */ },
      }
    }
  })
}

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
          if let Ok(Some(embeddings)) = indexer.embed_in_thread_pool(&embedder, chunks, &threads) {
            if let Err(err) = record_tx.send(EmbeddingRecord {
              workspace_id,
              object_id: object_id.clone(),
              tokens_used: embeddings.tokens_consumed,
              contents: embeddings.params,
            }) {
              error!("Failed to send embedding record: {}", err);
            }
          }
        }
      }
    });
  }
  Ok(())
}

const EMBEDDING_RECORD_BUFFER_SIZE: usize = 5;
async fn spawn_write_indexing(mut rx: UnboundedReceiver<EmbeddingRecord>, pg_pool: PgPool) {
  let mut buf = Vec::with_capacity(EMBEDDING_RECORD_BUFFER_SIZE);
  loop {
    let n = rx.recv_many(&mut buf, EMBEDDING_RECORD_BUFFER_SIZE).await;
    if n == 0 {
      info!("Stop writing embeddings");
      break;
    }

    let records = buf.drain(..n).collect::<Vec<_>>();
    match batch_insert_records(&pg_pool, records).await {
      Ok(_) => info!("wrote {} embedding records", n),
      Err(err) => error!("Failed to index collab {}", err),
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
      &record.object_id,
      record.tokens_used,
      record.contents,
    )
    .await?;
  }
  txn.commit().await?;
  Ok(())
}

fn process_collab(
  embdder: &Embedder,
  indexer_provider: &IndexerProvider,
  indexed_collab: &IndexedCollab,
  metrics: &EmbeddingMetrics,
) -> Result<Option<(u32, Vec<AFCollabEmbeddedChunk>)>, AppError> {
  if let Some(indexer) = indexer_provider.indexer_for(&indexed_collab.collab_type) {
    let start_time = Instant::now();
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

    let chunks = indexer.create_embedded_chunks(&collab, embdder.model())?;
    let result = indexer.embed(embdder, chunks);
    let duration = start_time.elapsed();
    metrics.record_processing_time(duration.as_millis());

    match result {
      Ok(Some(embeddings)) => {
        trace!(
          "Indexed collab {}, tokens: {}",
          indexed_collab.object_id,
          embeddings.tokens_consumed
        );
        Ok(Some((embeddings.tokens_consumed, embeddings.params)))
      },
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
