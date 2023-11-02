use collab::error::CollabError;

#[derive(Debug, thiserror::Error)]
pub enum RealtimeError {
  #[error(transparent)]
  YSync(#[from] collab::sync_protocol::message::Error),

  #[error(transparent)]
  YAwareness(#[from] collab::sync_protocol::awareness::Error),

  #[error("failed to deserialize message: {0}")]
  DecodingError(#[from] lib0::error::Error),

  #[error(transparent)]
  SerdeError(#[from] serde_json::Error),

  #[error(transparent)]
  TokioTask(#[from] tokio::task::JoinError),

  #[error(transparent)]
  IO(#[from] std::io::Error),

  #[error("Unexpected data: {0}")]
  UnexpectedData(&'static str),

  #[error(transparent)]
  CollabError(#[from] CollabError),

  #[error("Received message from client:{0}, but the client does not have sufficient permissions to write")]
  NotEnoughPermissionToWrite(i64),

  #[error("Client:{0} does not have enough permission to read")]
  NotEnoughPermissionToRead(i64),

  #[error("Internal failure: {0}")]
  Internal(#[from] anyhow::Error),
}

pub fn internal_error<T>(e: T) -> anyhow::Error
where
  T: std::error::Error + Send + Sync + 'static,
{
  anyhow::Error::from(e)
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum StreamError {
  #[error("Internal error:{0}")]
  Internal(String),
}
