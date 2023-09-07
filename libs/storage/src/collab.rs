use crate::entities::CreateCollab;
use anyhow::Result;
use sqlx::PgPool;

pub type CollabRawData = Vec<u8>;

pub struct CollabStorage {
  #[allow(dead_code)]
  pg_pool: PgPool,
}

impl CollabStorage {
  pub fn new(pg_pool: PgPool) -> Self {
    Self { pg_pool }
  }

  pub async fn is_exist(&self, _object_id: &str) -> bool {
    todo!()
  }

  pub async fn create_collab(&self, _params: CreateCollab) -> Result<()> {
    todo!()
  }

  pub async fn update_collab(&self, _object_id: &str, _data: CollabRawData) -> Result<()> {
    todo!()
  }

  pub async fn get_collab(&self, _object_id: &str) -> Result<Option<CollabRawData>> {
    todo!()
  }

  pub async fn delete_collab(&self, _object_id: &str) -> Result<()> {
    todo!()
  }
}
