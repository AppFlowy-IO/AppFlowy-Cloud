#[derive(Debug, thiserror::Error)]
pub enum CollabSyncError {
  #[error(transparent)]
  YSync(#[from] y_sync::sync::Error),

  #[error(transparent)]
  YAwareness(#[from] y_sync::awareness::Error),

  #[error("failed to deserialize message: {0}")]
  DecodingError(#[from] lib0::error::Error),

  #[error(transparent)]
  SerdeError(#[from] serde_json::Error),

  #[error(transparent)]
  TokioTask(#[from] tokio::task::JoinError),

  #[error(transparent)]
  IO(#[from] std::io::Error),

  #[error("Internal failure: {0}")]
  Internal(#[from] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum StreamError {
  #[error("Internal error:{0}")]
  Internal(String),
}
