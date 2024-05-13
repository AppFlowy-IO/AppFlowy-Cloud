#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error(transparent)]
  Stream(#[from] collab_stream::error::StreamError),
  #[error(transparent)]
  Collab(#[from] collab::error::CollabError),
  #[error(transparent)]
  AIClient(#[from] appflowy_ai_client::error::AIError),
  #[error("yrs update decode error: {0}")]
  UpdateDecode(#[from] yrs::encoding::read::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
