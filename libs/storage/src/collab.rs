use crate::entities::CreateCollabParams;
use crate::error::StorageError;
use sqlx::PgPool;

pub type Result<T, E = StorageError> = core::result::Result<T, E>;

pub type RawData = Vec<u8>;

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

  pub async fn create_collab(&self, _params: CreateCollabParams) -> Result<()> {
    todo!()
  }

  pub async fn update_collab(&self, _object_id: &str, _data: RawData) -> Result<()> {
    todo!()
  }

  pub async fn get_collab(&self, _object_id: &str) -> Result<RawData> {
    todo!()
  }

  pub async fn delete_collab(&self, _object_id: &str) -> Result<()> {
    todo!()
  }
}
