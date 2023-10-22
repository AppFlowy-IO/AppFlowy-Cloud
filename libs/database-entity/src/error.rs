use sqlx::Error;
use std::borrow::Cow;

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
  #[error("Record not found:{0}")]
  RecordNotFound(String),

  #[error(transparent)]
  UnexpectedData(#[from] validator::ValidationErrors),

  #[error(transparent)]
  IOError(#[from] std::io::Error),

  #[error(transparent)]
  UuidError(#[from] uuid::Error),

  #[error(transparent)]
  SqlxError(sqlx::Error),

  #[error("Storage space not enough")]
  StorageSpaceNotEnough,

  #[error("Bucket error:{0}")]
  BucketError(String),

  #[error("Not enough permission:{0}")]
  NotEnoughPermissions(String),

  #[error("Invalid params:{0}")]
  InvalidParams(String),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

impl DatabaseError {
  pub fn is_record_not_found(&self) -> bool {
    matches!(self, Self::RecordNotFound(_))
  }
}

impl From<sqlx::Error> for DatabaseError {
  fn from(value: sqlx::Error) -> Self {
    match value {
      Error::RowNotFound => {
        DatabaseError::RecordNotFound("Can't find the row in the database".to_string())
      },
      _ => DatabaseError::SqlxError(value),
    }
  }
}

impl From<DatabaseError> for Cow<'static, str> {
  fn from(value: DatabaseError) -> Self {
    Cow::Owned(format!("{:?}", value))
  }
}
