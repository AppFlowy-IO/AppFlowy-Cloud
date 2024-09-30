use app_error::AppError;
use chrono::NaiveDateTime;
use database_entity::dto::APIKeyPermission;
use sqlx::{Executor, Postgres};
use uuid::Uuid;

pub async fn insert_into_api_key<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
  api_key_hash: String,
  scopes: Vec<APIKeyPermission>,
  expiration_date: NaiveDateTime,
) -> Result<(), AppError> {
  let scopes: Vec<i16> = scopes.into_iter().map(|s| s as i16).collect();
  let res = sqlx::query!(
    r#"
      INSERT INTO af_api_key (
        workspace_id,
        uid,
        api_key_hash,
        scopes,
        expiration_date
      )
      VALUES ($1, (SELECT uid FROM af_user WHERE uuid = $2), $3, $4, $5)
    "#,
    workspace_id,
    user_uuid,
    api_key_hash,
    &scopes,
    expiration_date,
  )
  .execute(executor)
  .await?;

  if res.rows_affected() != 1 {
    tracing::error!(
      "Failed to insert api key, workspace_id: {}, user_uuid: {}",
      workspace_id,
      user_uuid
    );
  }

  Ok(())
}
