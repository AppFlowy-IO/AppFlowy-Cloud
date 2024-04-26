use collab_rt_protocol::RTProtocolError;
use std::fmt::Display;

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
    reason: MissUpdateReason,
  },

  #[error("Can not apply update")]
  CannotApplyUpdate,

  #[error("{0}")]
  OverrideWithIncorrectData(String),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

#[derive(Debug)]
pub enum MissUpdateReason {
  BroadcastSeqNotContinuous { current: u32, expected: u32 },
  AckSeqAdvanceBroadcastSeq { ack_seq: u32, broadcast_seq: u32 },
  ServerMissUpdates,
  Other(String),
}

impl Display for MissUpdateReason {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      MissUpdateReason::BroadcastSeqNotContinuous { current, expected } => {
        write!(
          f,
          "Broadcast sequence not continuous: current={}, expected={}",
          current, expected
        )
      },
      MissUpdateReason::AckSeqAdvanceBroadcastSeq {
        ack_seq,
        broadcast_seq,
      } => {
        write!(
          f,
          "Ack sequence advance broadcast sequence: ack_seq={}, broadcast_seq={}",
          ack_seq, broadcast_seq
        )
      },
      MissUpdateReason::ServerMissUpdates => write!(f, "Server miss updates"),
      MissUpdateReason::Other(reason) => write!(f, "{}", reason),
    }
  }
}

impl From<RTProtocolError> for SyncError {
  fn from(value: RTProtocolError) -> Self {
    match value {
      RTProtocolError::MissUpdates {
        state_vector_v1,
        reason,
      } => Self::MissUpdates {
        state_vector_v1,
        reason: MissUpdateReason::Other(reason),
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
