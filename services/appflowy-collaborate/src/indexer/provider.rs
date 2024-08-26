use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use actix::dev::Stream;
use async_stream::try_stream;
use async_trait::async_trait;
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use sqlx::PgPool;
use tokio_stream::StreamExt;
use uuid::Uuid;

use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
use database::collab::select_blob_from_af_collab;
use database::index::{get_collabs_without_embeddings, upsert_collab_embeddings};
use database::workspace::select_workspace_settings;
use database_entity::dto::{AFCollabEmbeddings, CollabParams};

use crate::indexer::DocumentIndexer;

#[async_trait]
pub trait Indexer: Send + Sync {
  async fn index(
    &self,
    object_id: &str,
    doc_state: Vec<u8>,
  ) -> Result<Option<AFCollabEmbeddings>, AppError>;
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

  pub async fn can_index_workspace(&self, workspace_id: &str) -> Result<bool, AppError> {
    let uuid = Uuid::parse_str(workspace_id)?;
    let settings = select_workspace_settings(&self.db, &uuid).await?;
    match settings {
      None => Ok(true),
      Some(settings) => Ok(!settings.disable_search_indexing),
    }
  }

  /// Returns indexer for a specific type of [Collab] object.
  /// If collab of given type is not supported or workspace it belongs to has indexing disabled,
  /// returns `None`.
  pub fn indexer_for(&self, collab_type: CollabType) -> Option<Arc<dyn Indexer>> {
    self.indexer_cache.get(&collab_type).cloned()
  }

  fn get_unindexed_collabs(
    &self,
  ) -> Pin<Box<dyn Stream<Item = Result<UnindexedCollab, anyhow::Error>> + Send>> {
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
            // only logging error in debug mode. Will be enabled in production if needed.
            if cfg!(debug_assertions) {
              tracing::warn!("failed to index collab {}/{}: {}", workspace, oid, err);
            }
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
      if let Some(embeddings) = indexer
        .index(&unindexed.object_id, unindexed.collab.doc_state.into())
        .await?
      {
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
    }
    Ok(())
  }

  pub async fn create_collab_embeddings(
    &self,
    params: &CollabParams,
  ) -> Result<Option<AFCollabEmbeddings>, AppError> {
    if let Some(indexer) = self.indexer_for(params.collab_type.clone()) {
      let embeddings = indexer
        .index(&params.object_id, params.encoded_collab_v1.clone())
        .await?;
      Ok(embeddings)
    } else {
      Ok(None)
    }
  }
}

struct UnindexedCollab {
  pub workspace_id: Uuid,
  pub object_id: String,
  pub collab_type: CollabType,
  pub collab: EncodedCollab,
}
