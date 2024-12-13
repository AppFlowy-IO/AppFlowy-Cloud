use crate::indexer::IndexerProvider;
use crate::state::RedisConnectionManager;
use actix::dev::Stream;
use anyhow::anyhow;
use app_error::AppError;
use async_stream::try_stream;
use bytes::Bytes;
use collab::entity::EncodedCollab;
use collab::lock::RwLock;
use collab::preclude::Collab;
use collab_entity::CollabType;
use database::collab::{CollabStorage, GetCollabOrigin};
use database::index::{get_collabs_without_embeddings, upsert_collab_embeddings};
use database::workspace::select_workspace_settings;
use database_entity::dto::CollabParams;
use futures_util::future::try_join_all;
use futures_util::StreamExt;
use redis::AsyncCommands;
use sqlx::PgPool;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tracing::trace;
use uuid::Uuid;

pub struct IndexerScheduler {
  indexer_provider: Arc<IndexerProvider>,
  pg_pool: PgPool,
  storage: Arc<dyn CollabStorage>,
}

impl IndexerScheduler {
  pub fn new(
    indexer_provider: Arc<IndexerProvider>,
    pg_pool: PgPool,
    storage: Arc<dyn CollabStorage>,
  ) -> Arc<Self> {
    let this = Arc::new(Self {
      indexer_provider,
      pg_pool,
      storage,
    });

    tokio::spawn(handle_unindexed_collabs(this.clone()));
    this
  }

  pub async fn index_encoded_collab(
    &self,
    workspace_id: &str,
    unindexed_collabs: Vec<UnindexedCollab>,
  ) -> Result<(), AppError> {
    let workspace_id = Uuid::parse_str(workspace_id)?;
    let mut futures = Vec::with_capacity(unindexed_collabs.len());
    for unindexed_collab in unindexed_collabs.into_iter() {
      let future = async move {
        if let Some(indexer) = self
          .indexer_provider
          .indexer_for(&unindexed_collab.collab_type)
        {
          let encoded_collab = tokio::task::spawn_blocking(move || {
            let encode_collab = EncodedCollab::decode_from_bytes(&unindexed_collab.encoded_collab)?;
            Ok::<_, AppError>(encode_collab)
          })
          .await??;
          let embeddings = indexer
            .index(&unindexed_collab.object_id, encoded_collab)
            .await?;
          Ok::<_, AppError>(embeddings)
        } else {
          Ok(None)
        }
      };

      futures.push(future);
    }

    if let Ok(results) = try_join_all(futures).await {
      for embeddings in results.into_iter() {
        if let Some(embeddings) = embeddings {
          upsert_collab_embeddings(
            &self.pg_pool,
            &workspace_id,
            embeddings.tokens_consumed,
            embeddings.params,
          )
          .await?;
        }
      }
    }

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
    let embedding_params = indexer.embedding_params(&lock).await?;
    drop(lock); // release the read lock ASAP

    let embeddings = indexer
      .embeddings(embedding_params)
      .await
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
        if let Err(err) =
          index_unindexd_collab(&scheduler.pg_pool, &scheduler.indexer_provider, collab).await
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
        tracing::error!("failed to get unindexed document: {}", err);
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
  unindexed: WorkspaceUnindexedCollab,
) -> Result<(), AppError> {
  if let Some(indexer) = indexer_provider.indexer_for(&unindexed.collab_type) {
    let workspace_id = unindexed.workspace_id;
    let embeddings = indexer
      .index(&unindexed.object_id, unindexed.collab)
      .await?;
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
