use crate::state::RedisClient;
use app_error::AppError;
use collab::core::collab_plugin::EncodedCollab;
use database::collab::CollabStoragePgImpl;
use database_entity::dto::QueryCollab;
use sqlx::PgPool;

pub struct CollabMemCache {
  pg_pool: PgPool,
  redis_client: RedisClient,
}

impl CollabMemCache {
  pub fn new(pg_pool: PgPool, redis_client: RedisClient) -> Self {
    Self {
      pg_pool,
      redis_client,
    }
  }

  pub async fn get_encoded_collab(&self, query: &QueryCollab) -> Option<EncodedCollab> {
    todo!()
  }

  pub async fn cache_encoded_collab(&self, collab: &EncodedCollab) {
    todo!()
  }
}
