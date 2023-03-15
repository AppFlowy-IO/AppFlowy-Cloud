#[derive(Debug, thiserror::Error)]
pub enum RevDBError {
    #[error(transparent)]
    Db(#[from] sled::Error),

    #[error("invalid data")]
    InvalidData,
}
