use anyhow::Error;
use collab_database::database::{Database, DatabaseContext};
use collab_database::entity::{CreateDatabaseParams, EncodedDatabase};
use collab_database::workspace_database::NoPersistenceDatabaseCollabService;
use std::sync::Arc;

pub async fn create_database_collab(
  params: CreateDatabaseParams,
) -> Result<EncodedDatabase, Error> {
  let collab_service = Arc::new(NoPersistenceDatabaseCollabService);
  let context = DatabaseContext {
    collab_service,
    notifier: Default::default(),
  };
  Database::create_with_view(params, context)
    .await?
    .encode_database_collabs()
    .await
    .map_err(|e| anyhow::anyhow!("Failed to encode database collabs: {:?}", e))
}
