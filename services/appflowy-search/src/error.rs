#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error(transparent)]
  SqlError(#[from] sqlx::Error),
  #[error(transparent)]
  RedisError(#[from] redis::RedisError),
  #[error(transparent)]
  SerializeError(#[from] serde_json::Error),
  #[error(transparent)]
  EmbeddingError(#[from] openai_api_rs::v1::error::APIError),
}
