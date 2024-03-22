#[derive(Debug, thiserror::Error)]
pub enum SyncError {
  #[error(transparent)]
  YSync(#[from] collab_rt_protocol::Error),

  #[error(transparent)]
  YAwareness(#[from] collab::core::awareness::Error),

  #[error("failed to deserialize message: {0}")]
  DecodingError(#[from] yrs::encoding::read::Error),

  #[error("Can not apply update for object:{0}")]
  CannotApplyUpdate(String),

  #[error(transparent)]
  SerdeError(#[from] serde_json::Error),

  #[error(transparent)]
  TokioTask(#[from] tokio::task::JoinError),

  #[error(transparent)]
  IO(#[from] std::io::Error),

  #[error("Workspace id is not found")]
  NoWorkspaceId,

  #[error("Missing broadcast data:{0}")]
  MissingBroadcast(String),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

impl SyncError {
  pub fn is_cannot_apply_update(&self) -> bool {
    matches!(self, Self::CannotApplyUpdate(_))
  }
}
