use async_trait::async_trait;
use collab::core::collab::CollabOptions;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::Collab;
use collab_database::error::DatabaseError;
use collab_database::workspace_database::{
  DatabaseCollabPersistenceService, DatabaseCollabService, EncodeCollabByOid,
};
use collab_entity::CollabType;
use database_entity::dto::QueryCollabResult::{Failed, Success};
use database_entity::dto::{QueryCollab, QueryCollabParams};
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

pub struct TestDatabaseCollabService {
  pub api_client: client_api::Client,
  pub workspace_id: Uuid,
}

#[async_trait]
impl DatabaseCollabService for TestDatabaseCollabService {
  async fn build_collab(
    &self,
    object_id: &str,
    object_type: CollabType,
    encoded_collab: Option<(EncodedCollab, bool)>,
  ) -> Result<Collab, DatabaseError> {
    let encoded_collab = match encoded_collab {
      None => {
        let params = QueryCollabParams {
          workspace_id: self.workspace_id,
          inner: QueryCollab {
            object_id: object_id.parse()?,
            collab_type: object_type,
          },
        };
        self
          .api_client
          .get_collab(params)
          .await
          .unwrap()
          .encode_collab
      },
      Some((encoded_collab, _)) => encoded_collab,
    };
    let options = CollabOptions::new(object_id.to_string()).with_data_source(encoded_collab.into());
    Ok(Collab::new_with_options(CollabOrigin::Empty, options).unwrap())
  }

  async fn finalize_collab(
    &self,
    _object_id: Uuid,
    _collab_type: CollabType,
    _collab: &mut Collab,
  ) -> Result<(), DatabaseError> {
    Ok(())
  }

  async fn get_collabs(
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
      .unwrap();
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

  fn persistence(&self) -> Option<Arc<dyn DatabaseCollabPersistenceService>> {
    None
  }
}
