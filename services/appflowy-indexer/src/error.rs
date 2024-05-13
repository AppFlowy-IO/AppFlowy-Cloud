#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error(transparent)]
  StreamError(#[from] collab_stream::error::StreamError),
  #[error(transparent)]
  CollabError(#[from] collab::error::CollabError),
  #[error("invalid workspace id: {0}")]
  InvalidWorkspaceId(String),
  #[error("yrs update decode error: {0}")]
  UpdateDecodeError(#[from] yrs::encoding::read::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
