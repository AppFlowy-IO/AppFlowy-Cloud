use sqlx::types::uuid;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
  #[error("Record not found")]
  RecordNotFound,

  #[error(transparent)]
  UnexpectedData(#[from] validator::ValidationErrors),

  #[error(transparent)]
  SqlxError(#[from] sqlx::Error),

  #[error(transparent)]
  UuidError(#[from] uuid::Error),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}
