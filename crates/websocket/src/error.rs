#[derive(Debug, thiserror::Error)]
pub enum WSError {
  #[error(transparent)]
  Persistence(#[from] collab_plugins::disk::error::PersistenceError),

  #[error("Internal failure: {0}")]
  Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}
