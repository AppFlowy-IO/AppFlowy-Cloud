use app_error::AppError;
use sqlx::{Executor, Postgres};
use uuid::Uuid;

pub async fn insert_into_api_key<'a, E: Executor<'a, Database = Postgres>>(
  _executor: E,
  _workspace_id: Uuid,
  _view_id: Uuid,
  _uid: i64,
) -> Result<Uuid, AppError> {
  todo!()
}
