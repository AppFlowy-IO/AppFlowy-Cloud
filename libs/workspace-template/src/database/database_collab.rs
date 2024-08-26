use anyhow::Error;
use async_trait::async_trait;
use collab::preclude::Collab;
use collab_database::database::{Database, DatabaseContext};
use collab_database::entity::{CreateDatabaseParams, EncodedDatabase};
use collab_database::error::DatabaseError;
use collab_database::workspace_database::{
  DatabaseCollabPersistenceService, DatabaseCollabService,
};
use collab_entity::CollabType;
use collab_folder::CollabOrigin;
use std::sync::Arc;
use std::vec;

struct TemplateDatabaseCollabServiceImpl;

#[async_trait]
impl DatabaseCollabService for TemplateDatabaseCollabServiceImpl {
  async fn build_collab(
    &self,
    object_id: &str,
    _object_type: CollabType,
    _is_new: bool,
  ) -> Result<Collab, DatabaseError> {
    Ok(Collab::new_with_origin(
      CollabOrigin::Empty,
      object_id,
      vec![],
      false,
    ))
  }

  fn persistence(&self) -> Option<Arc<dyn DatabaseCollabPersistenceService>> {
    None
  }
}

pub async fn create_database_collab(
  _object_id: String,
  params: CreateDatabaseParams,
) -> Result<EncodedDatabase, Error> {
  let collab_service = Arc::new(TemplateDatabaseCollabServiceImpl);
  let context = DatabaseContext {
    collab_service,
    notifier: Default::default(),
    is_new: true,
  };
  Database::create_with_view(params, context)
    .await?
    .encode_database_collabs()
    .await
    .map_err(|e| anyhow::anyhow!("Failed to encode database collabs: {:?}", e))
}
