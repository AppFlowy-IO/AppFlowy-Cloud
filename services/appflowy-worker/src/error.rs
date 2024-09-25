#[derive(thiserror::Error, Debug)]
pub enum WorkerError {
  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}
