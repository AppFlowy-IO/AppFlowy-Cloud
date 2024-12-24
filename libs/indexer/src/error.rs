#[derive(thiserror::Error, Debug)]
pub enum IndexerError {
  #[error("Redis stream group not exist: {0}")]
  StreamGroupNotExist(String),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}
