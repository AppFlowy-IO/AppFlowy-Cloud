use crate::config::get_env_var;
use crate::indexer::DocumentIndexer;
use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
use async_trait::async_trait;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::Collab;
use collab_entity::CollabType;
use database_entity::dto::{AFCollabEmbeddingParams, AFCollabEmbeddings};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

#[async_trait]
pub trait Indexer: Send + Sync {
  async fn embedding_params(
    &self,
    collab: &Collab,
  ) -> Result<Vec<AFCollabEmbeddingParams>, AppError>;

  async fn embedding_text(
    &self,
    object_id: String,
    content: String,
    collab_type: CollabType,
  ) -> Result<Vec<AFCollabEmbeddingParams>, AppError>;

  async fn embeddings(
    &self,
    params: Vec<AFCollabEmbeddingParams>,
  ) -> Result<Option<AFCollabEmbeddings>, AppError>;

  async fn index(
    &self,
    object_id: &str,
    encoded_collab: EncodedCollab,
  ) -> Result<Option<AFCollabEmbeddings>, AppError> {
    let collab = Collab::new_with_source(
      CollabOrigin::Empty,
      object_id,
      DataSource::DocStateV1(encoded_collab.doc_state.into()),
      vec![],
      false,
    )
    .map_err(|err| AppError::Internal(err.into()))?;
    let embedding_params = self.embedding_params(&collab).await?;
    self.embeddings(embedding_params).await
  }
}

/// A structure responsible for resolving different [Indexer] types for different [CollabType]s,
/// including access permission checks for the specific workspaces.
pub struct IndexerProvider {
  indexer_cache: HashMap<CollabType, Arc<dyn Indexer>>,
}

impl IndexerProvider {
  pub fn new(ai_client: AppFlowyAIClient) -> Arc<Self> {
    let mut cache: HashMap<CollabType, Arc<dyn Indexer>> = HashMap::new();
    let enabled = get_env_var("APPFLOWY_INDEXER_ENABLED", "true")
      .parse::<bool>()
      .unwrap_or(true);

    info!("Indexer is enabled: {}", enabled);
    if enabled {
      cache.insert(CollabType::Document, DocumentIndexer::new(ai_client));
    }
    Arc::new(Self {
      indexer_cache: cache,
    })
  }

  /// Returns indexer for a specific type of [Collab] object.
  /// If collab of given type is not supported or workspace it belongs to has indexing disabled,
  /// returns `None`.
  pub fn indexer_for(&self, collab_type: &CollabType) -> Option<Arc<dyn Indexer>> {
    self.indexer_cache.get(collab_type).cloned()
  }
}
