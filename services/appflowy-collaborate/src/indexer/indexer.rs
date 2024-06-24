use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use collab_entity::CollabType;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::indexer::DocumentIndexer;
use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
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
}
