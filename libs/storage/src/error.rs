#[derive(Debug, thiserror::Error)]
pub enum StorageError {
  #[error("Record not found")]
  RecordNotFound,

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}
