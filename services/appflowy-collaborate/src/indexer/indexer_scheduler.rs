use crate::indexer::IndexerProvider;
use crate::thread_pool_no_abort::{ThreadPoolNoAbort, ThreadPoolNoAbortBuilder};
use actix::dev::Stream;
use anyhow::anyhow;
use app_error::AppError;
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
use database_entity::dto::{AFCollabEmbeddingParams, AFCollabEmbeddings, CollabParams};
use futures_util::StreamExt;
use rayon::prelude::*;
use sqlx::PgPool;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, warn};
use uuid::Uuid;

pub struct IndexerScheduler {
  indexer_provider: Arc<IndexerProvider>,
  pg_pool: PgPool,
  storage: Arc<dyn CollabStorage>,
  threads: Arc<ThreadPoolNoAbort>,
}

impl IndexerScheduler {
  pub fn new(
    indexer_provider: Arc<IndexerProvider>,
    pg_pool: PgPool,
    storage: Arc<dyn CollabStorage>,
  ) -> Arc<Self> {
    // Since threads often block while waiting for I/O, you can use more threads than CPU cores to improve concurrency.
    // A good rule of thumb is 2x to 10x the number of CPU cores
    let threads = Arc::new(
      ThreadPoolNoAbortBuilder::new()
        .num_threads(20)
        .thread_name(|index| format!("embedding-request-{index}"))
        .build()
        .unwrap(),
    );

    let this = Arc::new(Self {
      indexer_provider,
      pg_pool,
      storage,
      threads,
    });

    tokio::spawn(handle_unindexed_collabs(this.clone()));
    this
  }

  pub fn index_encoded_collab_one(
    &self,
    workspace_id: &str,
    unindexed_collab: UnindexedCollab,
  ) -> Result<(), AppError> {
    let workspace_id = Uuid::parse_str(workspace_id)?;
    let indexer_provider = self.indexer_provider.clone();
    let pg_pool = self.pg_pool.clone();
    rayon::spawn(move || {
      if let Some((tokens_used, params)) = process_collab(&indexer_provider, &unindexed_collab) {
        tokio::spawn(async move {
          let result = upsert_collab_embeddings(&pg_pool, &workspace_id, tokens_used, params).await;
          if let Err(err) = result {
            warn!(
              "failed to index collab {}/{}: {}",
              workspace_id, unindexed_collab.object_id, err
            );
          }
        });
      } else {
        warn!("Failed to process collab for indexing");
      }
    });
    Ok(())
  }

  pub fn index_encoded_collabs(
    &self,
    workspace_id: &str,
    unindexed_collabs: Vec<UnindexedCollab>,
  ) -> Result<(), AppError> {
    let workspace_id = Uuid::parse_str(workspace_id)?;
    let indexer_provider = self.indexer_provider.clone();
    let threads = self.threads.clone();
    let pg_pool = self.pg_pool.clone();

    rayon::spawn(move || {
      let results = threads.install(|| {
        unindexed_collabs
          .into_par_iter()
          .filter_map(|collab| process_collab(&indexer_provider, &collab))
          .collect::<Vec<_>>()
      });

      match results {
        Ok(embeddings_list) => {
          tokio::spawn(async move {
            for (tokens_used, params) in embeddings_list {
              let result =
                upsert_collab_embeddings(&pg_pool, &workspace_id, tokens_used, params).await;
              if let Err(err) = result {
                error!("Failed to index collab in batch: {}", err);
              }
            }
          });
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
    let workspace_id = Uuid::parse_str(workspace_id)?;
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
    let embedding_params = indexer.embedding_collab(&lock)?;
    drop(lock); // release the read lock ASAP

    let embeddings = indexer
      .embed_in_thread_pool(embedding_params, &self.threads)
      .unwrap()
      .ok_or_else(|| {
        AppError::Internal(anyhow!(
          "Failed to get embeddings for collab {:?}",
          object_id
        ))
      })?;

    upsert_collab_embeddings(
      &self.pg_pool,
      &workspace_id,
      embeddings.tokens_consumed,
      embeddings.params.clone(),
    )
    .await?;
    Ok(())
  }

  pub async fn can_index_workspace(&self, workspace_id: &str) -> Result<bool, AppError> {
    let uuid = Uuid::parse_str(workspace_id)?;
    let settings = select_workspace_settings(&self.pg_pool, &uuid).await?;
    match settings {
      None => Ok(true),
      Some(settings) => Ok(!settings.disable_search_indexing),
    }
  }
}

async fn handle_unindexed_collabs(scheduler: Arc<IndexerScheduler>) {
  let start = Instant::now();
  let mut i = 0;
  let mut stream = get_unindexed_collabs(&scheduler.pg_pool, scheduler.storage.clone());
  while let Some(result) = stream.next().await {
    match result {
      Ok(collab) => {
        let workspace = collab.workspace_id;
        let oid = collab.object_id.clone();
        if let Err(err) = index_unindexd_collab(
          &scheduler.pg_pool,
          &scheduler.indexer_provider,
          &scheduler.threads,
          collab,
        )
        .await
        {
          // only logging error in debug mode. Will be enabled in production if needed.
          if cfg!(debug_assertions) {
            tracing::warn!("failed to index collab {}/{}: {}", workspace, oid, err);
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
  tracing::info!(
    "indexed {} unindexed collabs in {:?} after restart",
    i,
    start.elapsed()
  );
}

fn get_unindexed_collabs(
  pg_pool: &PgPool,
  storage: Arc<dyn CollabStorage>,
) -> Pin<Box<dyn Stream<Item = Result<WorkspaceUnindexedCollab, anyhow::Error>> + Send>> {
  let db = pg_pool.clone();
  Box::pin(try_stream! {
    let collabs = get_collabs_without_embeddings(&db).await?;
    if !collabs.is_empty() {
      tracing::info!("found {} unindexed collabs", collabs.len());
    }
    for cid in collabs {
      match &cid.collab_type {
        CollabType::Document => {
          let collab = storage
            .get_encode_collab(GetCollabOrigin::Server, cid.clone().into(), false)
            .await?;

          yield WorkspaceUnindexedCollab {
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
  pg_pool: &PgPool,
  indexer_provider: &Arc<IndexerProvider>,
  threads: &ThreadPoolNoAbort,
  unindexed: WorkspaceUnindexedCollab,
) -> Result<(), AppError> {
  if let Some(indexer) = indexer_provider.indexer_for(&unindexed.collab_type) {
    let workspace_id = unindexed.workspace_id;
    let collab = Collab::new_with_source(
      CollabOrigin::Empty,
      &unindexed.object_id,
      DataSource::DocStateV1(unindexed.collab.doc_state.into()),
      vec![],
      false,
    )
    .map_err(|err| AppError::Internal(err.into()))?;

    let embedding_params = indexer.embedding_collab(&collab)?;
    let embeddings = indexer.embed_in_thread_pool(embedding_params, threads)?;
    if let Some(embeddings) = embeddings {
      upsert_collab_embeddings(
        pg_pool,
        &workspace_id,
        embeddings.tokens_consumed,
        embeddings.params,
      )
      .await?;
    }
  }
  Ok(())
}

fn process_collab(
  indexer_provider: &IndexerProvider,
  unindexed_collab: &UnindexedCollab,
) -> Option<(u32, Vec<AFCollabEmbeddingParams>)> {
  let indexer = indexer_provider.indexer_for(&unindexed_collab.collab_type)?;
  let encode_collab = EncodedCollab::decode_from_bytes(&unindexed_collab.encoded_collab).ok()?;
  let collab = Collab::new_with_source(
    CollabOrigin::Empty,
    &unindexed_collab.object_id,
    DataSource::DocStateV1(encode_collab.doc_state.into()),
    vec![],
    false,
  )
  .ok()?;
  let params = indexer.embedding_collab(&collab).ok()?;
  let embeddings = indexer.embed(params).ok()??;

  Some((embeddings.tokens_consumed, embeddings.params))
}

pub struct WorkspaceUnindexedCollab {
  pub workspace_id: Uuid,
  pub object_id: String,
  pub collab_type: CollabType,
  pub collab: EncodedCollab,
}

pub struct UnindexedCollab {
  pub object_id: String,
  pub collab_type: CollabType,
  pub encoded_collab: Bytes,
}

impl From<&CollabParams> for UnindexedCollab {
  fn from(params: &CollabParams) -> Self {
    Self {
      object_id: params.object_id.clone(),
      collab_type: params.collab_type.clone(),
      encoded_collab: params.encoded_collab_v1.clone(),
    }
  }
}
