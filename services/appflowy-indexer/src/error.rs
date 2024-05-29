#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error(transparent)]
  Stream(#[from] collab_stream::error::StreamError),
  #[error(transparent)]
  Collab(#[from] collab::error::CollabError),
  #[error("yrs update decode error: {0}")]
  UpdateDecode(#[from] yrs::encoding::read::Error),
  #[error("collab document decoding error: {0}")]
  Document(#[from] collab_document::error::DocumentError),
  #[error("couldn't decode JSON: {0}")]
  Serde(#[from] serde_json::Error),
  #[error(transparent)]
  Sql(#[from] sqlx::Error),
  #[error("OpenAI failed to process request: {0}")]
  OpenAI(String),
  #[error("invalid workspace ID: {0}")]
  InvalidWorkspace(uuid::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
