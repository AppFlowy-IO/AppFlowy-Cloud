#[derive(Debug, thiserror::Error)]
pub enum WSError {
  #[error("Internal failure: {0}")]
  Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}
