#[derive(Debug, thiserror::Error)]
pub enum EntityError {
  #[error("Invalid data: {0}")]
  InvalidData(String),
  #[error("Deserialization error: {0}")]
  DeserializationError(String),
}
