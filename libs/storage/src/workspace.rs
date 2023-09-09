use crate::error::StorageError;
use sqlx::{types::uuid, Postgres, Transaction};

pub type Result<T, E = StorageError> = core::result::Result<T, E>;

pub type RawData = Vec<u8>;

pub async fn create_workspace_if_not_exists(
  _trans: Transaction<'_, Postgres>,
  _owner_uid: uuid::Uuid,
) -> Result<uuid::Uuid> {
  // let _ = sqlx::query!(
  //   r#"
  //      INSERT INTO af_workspace (owner_uid)
  //      VALUES ($1)
  //      ON CONFLICT (owner_uid) DO NOTHING
  //      RETURNING id
  //    "#,
  // )
  // .execute(trans)
  // .await
  // .context("Save user to disk failed")
  // .map_err(internal_error)?;

  todo!()
}
