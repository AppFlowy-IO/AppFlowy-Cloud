use collab_rt_protocol::RTProtocolError;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
  #[error(transparent)]
  YSync(RTProtocolError),

  #[error(transparent)]
  YAwareness(#[from] collab::core::awareness::Error),

  #[error("failed to deserialize message: {0}")]
  DecodingError(#[from] yrs::encoding::read::Error),

  #[error("Can not apply update for object:{0}")]
  YrsApplyUpdate(String),

  #[error(transparent)]
  SerdeError(#[from] serde_json::Error),

  #[error(transparent)]
  TokioTask(#[from] tokio::task::JoinError),

  #[error(transparent)]
  IO(#[from] std::io::Error),

  #[error("Workspace id is not found")]
  NoWorkspaceId,

  #[error("Missing updates")]
  MissUpdates {
    state_vector_v1: Option<Vec<u8>>,
    reason: String,
  },

  #[error("Can not apply update")]
  CannotApplyUpdate,

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

impl From<RTProtocolError> for SyncError {
  fn from(value: RTProtocolError) -> Self {
    match value {
      RTProtocolError::MissUpdates {
        state_vector_v1,
        reason,
      } => Self::MissUpdates {
        state_vector_v1,
        reason,
      },
      RTProtocolError::DecodingError(e) => Self::DecodingError(e),
      RTProtocolError::YAwareness(e) => Self::YAwareness(e),
      RTProtocolError::YrsApplyUpdate(e) => Self::YrsApplyUpdate(e),
      RTProtocolError::Internal(e) => Self::Internal(e),
      _ => Self::YSync(value),
    }
  }
}

impl SyncError {
  pub fn is_cannot_apply_update(&self) -> bool {
    matches!(self, Self::YrsApplyUpdate(_))
  }
}
