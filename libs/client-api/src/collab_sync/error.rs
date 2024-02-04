#[derive(Debug, thiserror::Error)]
pub enum SyncError {
  #[error(transparent)]
  YSync(#[from] realtime_protocol::Error),

  #[error(transparent)]
  YAwareness(#[from] collab::core::awareness::Error),

  #[error("failed to deserialize message: {0}")]
  DecodingError(#[from] yrs::encoding::read::Error),

  #[error(transparent)]
  SerdeError(#[from] serde_json::Error),

  #[error(transparent)]
  TokioTask(#[from] tokio::task::JoinError),

  #[error(transparent)]
  IO(#[from] std::io::Error),

  #[error("Workspace id is not found")]
  NoWorkspaceId,

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}
