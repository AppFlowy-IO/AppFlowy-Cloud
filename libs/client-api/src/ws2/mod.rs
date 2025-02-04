mod controller;

pub type WorkspaceId = uuid::Uuid;
pub type Oid = uuid::Uuid;
pub use controller::{Options, WorkspaceController};

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
  #[error("WebSocket error: {0}")]
  Ws(#[from] tokio_tungstenite::tungstenite::Error),
  #[error("HTTP error: {0}")]
  Http(#[from] tokio_tungstenite::tungstenite::http::Error),
  #[error("collab decoding error: {0}")]
  YrsDecoding(#[from] yrs::encoding::read::Error),
}
