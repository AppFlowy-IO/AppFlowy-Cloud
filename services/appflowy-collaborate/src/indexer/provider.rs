use actix::dev::Stream;
use async_stream::try_stream;
use async_trait::async_trait;
use collab::core::collab::{DataSource, MutexCollab};
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::Collab;
use collab_entity::CollabType;
use sqlx::PgPool;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::indexer::DocumentIndexer;
use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
use database::collab::select_blob_from_af_collab;
use database::index::{get_collabs_without_embeddings, upsert_collab_embeddings};
use database::workspace::select_workspace_settings;
use database_entity::dto::AFCollabEmbeddings;

#[async_trait]
pub trait Indexer: Send + Sync {
  async fn index(&self, collab: MutexCollab) -> Result<AFCollabEmbeddings, AppError>;
}

/// A structure responsible for resolving different [Indexer] types for different [CollabType]s,
/// including access permission checks for the specific workspaces.
pub struct IndexerProvider {
  db: PgPool,
  indexer_cache: HashMap<CollabType, Arc<dyn Indexer>>,
}

impl IndexerProvider {
  pub fn new(db: PgPool, ai_client: AppFlowyAIClient) -> Arc<Self> {
    let mut cache: HashMap<CollabType, Arc<dyn Indexer>> = HashMap::new();
    cache.insert(CollabType::Document, DocumentIndexer::new(ai_client));
    Arc::new(Self {
      db,
      indexer_cache: cache,
    })
  }

  /// Returns indexer for a specific type of [Collab] object.
  /// If collab of given type is not supported or workspace it belongs to has indexing disabled,
  /// returns `None`.
  pub async fn indexer_for(
    &self,
    workspace_id: &str,
    collab_type: CollabType,
  ) -> Result<Option<Arc<dyn Indexer>>, AppError> {
    let indexer = self.indexer_cache.get(&collab_type).cloned();
    if indexer.is_none() {
      return Ok(None);
    }
    let uuid = Uuid::parse_str(workspace_id)?;
    let settings = select_workspace_settings(&self.db, &uuid).await?;
    match settings {
      Some(settings) if settings.disable_search_indexing => Ok(None),
      _ => Ok(indexer),
    }
  }

  fn get_unindexed_collabs(
    &self,
  ) -> Pin<Box<dyn Stream<Item = Result<UnindexedCollab, anyhow::Error>>>> {
    let db = self.db.clone();
    Box::pin(try_stream! {
      let collabs = get_collabs_without_embeddings(&db).await?;

      if !collabs.is_empty() {
        tracing::trace!("found {} unindexed collabs", collabs.len());
      }
      for cid in collabs {
        match &cid.collab_type {
          CollabType::Document => {
            let collab =
              select_blob_from_af_collab(&db, &CollabType::Document, &cid.object_id).await?;
            let collab = EncodedCollab::decode_from_bytes(&collab)?;
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

  pub async fn handle_unindexed_collabs(indexer: Arc<Self>) {
    let mut stream = indexer.get_unindexed_collabs();
    while let Some(result) = stream.next().await {
      match result {
        Ok(collab) => {
          let workspace = collab.workspace_id;
          let oid = collab.object_id.clone();
          if let Err(err) = Self::index_collab(&indexer, collab).await {
            tracing::warn!("failed to index collab {}/{}: {}", workspace, oid, err);
          }
        },
        Err(err) => {
          tracing::error!("failed to get unindexed document: {}", err);
        },
      }
    }
  }

  async fn index_collab(&self, unindexed: UnindexedCollab) -> Result<(), AppError> {
    if let Some(indexer) = self.indexer_cache.get(&unindexed.collab_type) {
      let collab = MutexCollab::new(
        Collab::new_with_source(
          CollabOrigin::Empty,
          &unindexed.object_id,
          DataSource::DocStateV1(unindexed.collab.doc_state.into()),
          vec![],
          false,
        )
        .map_err(|err| AppError::Internal(err.into()))?,
      );
      let embeddings = indexer.index(collab).await?;
      let mut tx = self.db.begin().await?;
      upsert_collab_embeddings(
        &mut tx,
        &unindexed.workspace_id,
        embeddings.tokens_consumed,
        &embeddings.params,
      )
      .await?;
      tx.commit().await?;
    }
    Ok(())
  }
}

struct UnindexedCollab {
  pub workspace_id: Uuid,
  pub object_id: String,
  pub collab_type: CollabType,
  pub collab: EncodedCollab,
}
