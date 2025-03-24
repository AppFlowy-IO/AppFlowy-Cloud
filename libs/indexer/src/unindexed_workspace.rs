use crate::collab_indexer::IndexerProvider;
use crate::entity::{EmbeddingRecord, UnindexedCollab};
use crate::scheduler::{batch_insert_records, IndexerScheduler};
use crate::thread_pool::ThreadPoolNoAbort;
use crate::vector::embedder::Embedder;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use database::collab::{CollabStorage, GetCollabOrigin};
use database::index::stream_collabs_without_embeddings;
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use rayon::iter::ParallelIterator;
use rayon::prelude::IntoParallelIterator;
use sqlx::pool::PoolConnection;
use sqlx::Postgres;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, trace};
use uuid::Uuid;

#[allow(dead_code)]
pub(crate) async fn index_workspace(scheduler: Arc<IndexerScheduler>, workspace_id: Uuid) {
  let weak_threads = Arc::downgrade(&scheduler.threads);
  let mut retry_delay = Duration::from_secs(2);
  loop {
    let threads = match weak_threads.upgrade() {
      Some(threads) => threads,
      None => {
        info!("[Embedding] thread pool is dropped, stop indexing");
        break;
      },
    };

    let conn = scheduler.pg_pool.try_acquire();
    if conn.is_none() {
      tokio::time::sleep(retry_delay).await;
      // 4s, 8s, 16s, 32s, 60s
      retry_delay = retry_delay.saturating_mul(2);
      if retry_delay > Duration::from_secs(60) {
        error!("[Embedding] failed to acquire db connection for 1 minute, stop indexing");
        break;
      }
      continue;
    }

    retry_delay = Duration::from_secs(2);
    let mut conn = conn.unwrap();
    let mut stream =
      stream_unindexed_collabs(&mut conn, workspace_id, scheduler.storage.clone(), 50).await;

    let batch_size = 5;
    let mut unindexed_collabs = Vec::with_capacity(batch_size);
    while let Some(Ok(collab)) = stream.next().await {
      if unindexed_collabs.len() < batch_size {
        unindexed_collabs.push(collab);
        continue;
      }

      index_then_write_embedding_to_disk(
        &scheduler,
        threads.clone(),
        std::mem::take(&mut unindexed_collabs),
      )
      .await;
    }

    if !unindexed_collabs.is_empty() {
      index_then_write_embedding_to_disk(&scheduler, threads.clone(), unindexed_collabs).await;
    }
  }
}

async fn index_then_write_embedding_to_disk(
  scheduler: &Arc<IndexerScheduler>,
  threads: Arc<ThreadPoolNoAbort>,
  unindexed_collabs: Vec<UnindexedCollab>,
) {
  info!(
    "[Embedding] process batch {:?} embeddings",
    unindexed_collabs
      .iter()
      .map(|v| v.object_id.clone())
      .collect::<Vec<_>>()
  );

  if let Ok(embedder) = scheduler.create_embedder() {
    let start = Instant::now();
    let embeddings = create_embeddings(
      embedder,
      &scheduler.indexer_provider,
      threads.clone(),
      unindexed_collabs,
    )
    .await;
    scheduler
      .metrics
      .record_gen_embedding_time(embeddings.len() as u32, start.elapsed().as_millis());

    let write_start = Instant::now();
    let n = embeddings.len();
    match batch_insert_records(&scheduler.pg_pool, embeddings).await {
      Ok(_) => trace!(
        "[Embedding] upsert {} embeddings success, cost:{}ms",
        n,
        write_start.elapsed().as_millis()
      ),
      Err(err) => error!("{}", err),
    }

    scheduler
      .metrics
      .record_write_embedding_time(write_start.elapsed().as_millis());
    tokio::time::sleep(Duration::from_secs(5)).await;
  } else {
    trace!("[Embedding] no embeddings to process in this batch");
  }
}

async fn stream_unindexed_collabs(
  conn: &mut PoolConnection<Postgres>,
  workspace_id: Uuid,
  storage: Arc<dyn CollabStorage>,
  limit: i64,
) -> BoxStream<Result<UnindexedCollab, anyhow::Error>> {
  let cloned_storage = storage.clone();
  stream_collabs_without_embeddings(conn, workspace_id, limit)
    .await
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
            // TODO(nathan): support other collab types
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

async fn create_embeddings(
  embedder: Embedder,
  indexer_provider: &Arc<IndexerProvider>,
  threads: Arc<ThreadPoolNoAbort>,
  unindexed_records: Vec<UnindexedCollab>,
) -> Vec<EmbeddingRecord> {
  unindexed_records
    .into_par_iter()
    .flat_map(|unindexed| {
      let indexer = indexer_provider.indexer_for(unindexed.collab_type)?;
      let collab = Collab::new_with_source(
        CollabOrigin::Empty,
        &unindexed.object_id,
        DataSource::DocStateV1(unindexed.collab.doc_state.into()),
        vec![],
        false,
      )
      .ok()?;

      let chunks = indexer
        .create_embedded_chunks_from_collab(&collab, embedder.model())
        .ok()?;
      if chunks.is_empty() {
        trace!("[Embedding] {} has no embeddings", unindexed.object_id,);
        return Some(EmbeddingRecord::empty(
          unindexed.workspace_id,
          unindexed.object_id,
          unindexed.collab_type,
        ));
      }

      let result = threads.install(|| match indexer.embed(&embedder, chunks) {
        Ok(embeddings) => embeddings.map(|embeddings| EmbeddingRecord {
          workspace_id: unindexed.workspace_id,
          object_id: unindexed.object_id,
          collab_type: unindexed.collab_type,
          tokens_used: embeddings.tokens_consumed,
          contents: embeddings.params,
        }),
        Err(err) => {
          error!("Failed to embed collab: {}", err);
          None
        },
      });

      if let Ok(Some(record)) = &result {
        trace!(
          "[Embedding] generate collab:{} embeddings, tokens used: {}",
          record.object_id,
          record.tokens_used
        );
      }

      result.unwrap_or_else(|err| {
        error!("Failed to spawn a task to index collab: {}", err);
        None
      })
    })
    .collect::<Vec<_>>()
}
