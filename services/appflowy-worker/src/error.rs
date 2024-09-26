#[derive(thiserror::Error, Debug)]
pub enum WorkerError {
  #[error(transparent)]
  ZipError(#[from] async_zip::error::ZipError),

  #[error("Record not found: {0}")]
  RecordNotFound(String),

  #[error(transparent)]
  IOError(#[from] std::io::Error),

  #[error(transparent)]
  ImportError(#[from] collab_importer::error::ImporterError),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}
