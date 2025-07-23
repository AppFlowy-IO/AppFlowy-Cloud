use async_trait::async_trait;
use collab::entity::EncodedCollab;
use collab::lock::RwLock;
use collab::preclude::ClientID;
use collab_database::database_trait::{DatabaseCollabReader, EncodeCollabByOid};
use collab_database::error::DatabaseError;
use collab_database::rows::{DatabaseRow, RowId};
use collab_entity::CollabType;
use dashmap::DashMap;
use database_entity::dto::QueryCollabResult::{Failed, Success};
use database_entity::dto::{QueryCollab, QueryCollabParams};
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

pub struct TestDatabaseCollabService {
  pub api_client: client_api::Client,
  pub workspace_id: Uuid,
  pub client_id: ClientID,
  cache: Arc<DashMap<RowId, Arc<RwLock<DatabaseRow>>>>,
}

impl TestDatabaseCollabService {
  pub fn new(api_client: client_api::Client, workspace_id: Uuid, client_id: ClientID) -> Self {
    Self {
      api_client,
      workspace_id,
      client_id,
      cache: Arc::new(DashMap::new()),
    }
  }
}

#[async_trait]
impl DatabaseCollabReader for TestDatabaseCollabService {
  async fn reader_client_id(&self) -> ClientID {
    self.client_id
  }

  async fn reader_get_collab(
    &self,
    object_id: &str,
    collab_type: CollabType,
  ) -> Result<EncodedCollab, DatabaseError> {
    let object_id = Uuid::parse_str(object_id)?;
    let params = QueryCollabParams {
      workspace_id: self.workspace_id,
      inner: QueryCollab {
        object_id,
        collab_type,
      },
    };
    let resp = self
      .api_client
      .get_collab(params)
      .await
      .map_err(|err| DatabaseError::Internal(err.into()))?;
    Ok(resp.encode_collab)
  }

  async fn reader_batch_get_collabs(
    &self,
    object_ids: Vec<String>,
    collab_type: CollabType,
  ) -> Result<EncodeCollabByOid, DatabaseError> {
    let params = object_ids
      .into_iter()
      .flat_map(|object_id| match Uuid::parse_str(&object_id) {
        Ok(object_id) => Ok(QueryCollab::new(object_id, collab_type)),
        Err(err) => Err(err),
      })
      .collect();
    let results = self
      .api_client
      .batch_get_collab(&self.workspace_id, params)
      .await
      .map_err(|err| DatabaseError::Internal(err.into()))?;
    Ok(
      results
        .0
        .into_iter()
        .flat_map(|(object_id, result)| match result {
          Success { encode_collab_v1 } => match EncodedCollab::decode_from_bytes(&encode_collab_v1)
          {
            Ok(encode) => Some((object_id.to_string(), encode)),
            Err(err) => {
              error!("Failed to decode collab: {}", err);
              None
            },
          },
          Failed { error } => {
            error!("Failed to get {} update: {}", object_id, error);
            None
          },
        })
        .collect::<EncodeCollabByOid>(),
    )
  }

  fn database_row_cache(&self) -> Option<Arc<DashMap<RowId, Arc<RwLock<DatabaseRow>>>>> {
    Some(self.cache.clone())
  }
}
