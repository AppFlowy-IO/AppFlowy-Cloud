#[derive(Debug, thiserror::Error)]
pub enum RevDBError {
    #[error(transparent)]
    Db(#[from] sled::Error),

    #[error("Serde error")]
    SerdeError,

    #[error("invalid data")]
    InvalidData,
}
