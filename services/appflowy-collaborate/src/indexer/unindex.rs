use crate::config::get_env_var;
use crate::indexer::vector::embedder::Embedder;
use crate::indexer::{batch_insert_records, EmbeddingRecord, IndexerProvider, IndexerScheduler};
use crate::thread_pool_no_abort::ThreadPoolNoAbort;
use app_error::AppError;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::Collab;
use collab_entity::CollabType;
use database::collab::{CollabStorage, GetCollabOrigin};
use database::index::{get_collabs_without_embeddings, get_collabs_without_embeddings_stream};
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use rayon::iter::ParallelIterator;
use rayon::prelude::IntoParallelIterator;
use sqlx::pool::PoolConnection;
use sqlx::Postgres;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{error, info, trace};
use uuid::Uuid;

/// Periodically checks for unindexed collabs and indexes them
pub(crate) async fn handle_unindexed_collabs_periodically(scheduler: Arc<IndexerScheduler>) {
  // Wait for 30 seconds before starting indexing
  tokio::time::sleep(Duration::from_secs(30)).await;

  let limit = get_env_var("APPFLOWY_INDEXER_SCHEDULER_GET_UNINDEXED_LIMIT", "50")
    .parse::<i64>()
    .unwrap_or(50);

  let weak_threads = Arc::downgrade(&scheduler.threads);
  let mut retry_delay = Duration::from_secs(5);

  loop {
    let handle_batch_start = Instant::now();
    let threads = match weak_threads.upgrade() {
      Some(threads) => threads,
      None => {
        info!("[Embedding] thread pool is dropped, stop indexing");
        break;
      },
    };

    let conn = scheduler.pg_pool.try_acquire();
    let unindexed_collabs = match conn {
      None => {
        tokio::time::sleep(retry_delay).await;
        retry_delay = retry_delay.saturating_mul(2);
        continue;
      },
      Some(mut conn) => {
        retry_delay = Duration::from_secs(5);
        get_unindexed_collabs(&mut conn, limit, scheduler.storage.clone())
          .await
          .unwrap_or_default()
      },
    };

    info!(
      "[Embedding] process batch {:?} embeddings",
      unindexed_collabs
        .iter()
        .map(|v| v.object_id.clone())
        .collect::<Vec<_>>()
    );

    if unindexed_collabs.is_empty() {
      trace!("[Embedding] no embeddings to process in this batch");
      tokio::time::sleep(Duration::from_secs(10)).await;
    } else if let Ok(embedder) = scheduler.create_embedder() {
      let indexed_collabs = index_unindexd_collab(
        embedder,
        &scheduler.indexer_provider,
        threads,
        unindexed_collabs,
      )
      .await;

      let write_start = Instant::now();
      let n = indexed_collabs.len();
      match batch_insert_records(&scheduler.pg_pool, indexed_collabs).await {
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
      scheduler.metrics.record_handle_batch_unindexed_collab_time(
        n as u32,
        handle_batch_start.elapsed().as_millis(),
      );
      tokio::time::sleep(Duration::from_secs(5)).await;
    } else {
      trace!("[Embedding] no embeddings to process in this batch");
    }
  }
}

#[allow(dead_code)]
async fn get_unindexed_collabs_stream(
  conn: &mut PoolConnection<Postgres>,
  limit: i64,
  storage: Arc<dyn CollabStorage>,
) -> BoxStream<Result<UnindexedCollab, anyhow::Error>> {
  let cloned_storage = storage.clone();
  get_collabs_without_embeddings_stream(conn, limit)
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
async fn get_unindexed_collabs(
  conn: &mut PoolConnection<Postgres>,
  limit: i64,
  storage: Arc<dyn CollabStorage>,
) -> Result<Vec<UnindexedCollab>, anyhow::Error> {
  let collab_ids = get_collabs_without_embeddings(conn, limit).await?;
  let mut result = Vec::new();
  for cid in collab_ids {
    if cid.collab_type == CollabType::Document {
      // Get the collaboration details
      let collab = storage
        .get_encode_collab(GetCollabOrigin::Server, cid.clone().into(), false)
        .await?;

      // Add the result to the vector
      result.push(UnindexedCollab {
        workspace_id: cid.workspace_id,
        object_id: cid.object_id,
        collab_type: cid.collab_type,
        collab,
      });
    }
  }
  Ok(result)
}
async fn index_unindexd_collab(
  embedder: Embedder,
  indexer_provider: &Arc<IndexerProvider>,
  threads: Arc<ThreadPoolNoAbort>,
  unindexed_records: Vec<UnindexedCollab>,
) -> Vec<EmbeddingRecord> {
  unindexed_records
    .into_par_iter()
    .flat_map(|unindexed| {
      let indexer = indexer_provider.indexer_for(&unindexed.collab_type)?;
      let collab = Collab::new_with_source(
        CollabOrigin::Empty,
        &unindexed.object_id,
        DataSource::DocStateV1(unindexed.collab.doc_state.into()),
        vec![],
        false,
      )
      .ok()?;

      let chunks = indexer
        .create_embedded_chunks(&collab, embedder.model())
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

pub struct UnindexedCollab {
  pub workspace_id: Uuid,
  pub object_id: String,
  pub collab_type: CollabType,
  pub collab: EncodedCollab,
}
