use collab::error::CollabError;

#[derive(Debug, thiserror::Error)]
pub enum RealtimeError {
  #[error(transparent)]
  YSync(#[from] collab_rt_protocol::RTProtocolError),

  #[error(transparent)]
  YAwareness(#[from] collab::core::awareness::Error),

  #[error("failed to deserialize message: {0}")]
  YrsDecodingError(#[from] yrs::encoding::read::Error),

  #[error(transparent)]
  SerdeError(#[from] serde_json::Error),

  #[error(transparent)]
  TokioTask(#[from] tokio::task::JoinError),

  #[error(transparent)]
  IO(#[from] std::io::Error),

  #[error("Unexpected data: {0}")]
  UnexpectedData(&'static str),

  #[error("Expected init sync message, but received: {0}")]
  ExpectInitSync(String),

  #[error(transparent)]
  CollabError(#[from] CollabError),

  #[error("Received message from client:{0}, but the client does not have sufficient permissions to write")]
  NotEnoughPermissionToWrite(i64),

  #[error("Client:{0} does not have enough permission to read")]
  NotEnoughPermissionToRead(i64),

  #[error("{0}")]
  UserNotFound(String),

  #[error("group is not exist: {0}")]
  GroupNotFound(String),

  #[error("Lack of required collab data: {0}")]
  NoRequiredCollabData(String),

  #[error("Internal failure: {0}")]
  Internal(#[from] anyhow::Error),
}
