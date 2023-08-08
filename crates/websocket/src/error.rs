#[derive(Debug, Clone, thiserror::Error)]
pub enum WSError {
  #[error("Internal failure:{0}")]
  Internal(String),
}
