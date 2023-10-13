use anyhow::Context;
use database_entity::database_error::DatabaseError;
use sqlx::PgPool;
use tracing::instrument;

pub async fn update_user_name(
  pool: &PgPool,
  uuid: &uuid::Uuid,
  name: &str,
) -> Result<(), DatabaseError> {
  sqlx::query!(
    r#"
        UPDATE af_user
        SET name = $1
        WHERE uuid = $2
        "#,
    name,
    uuid
  )
  .execute(pool)
  .await?;
  Ok(())
}

#[instrument(skip_all, err)]
pub async fn create_user_if_not_exists(
  pool: &PgPool,
  user_uuid: &uuid::Uuid,
  email: &str,
  name: &str,
) -> Result<bool, DatabaseError> {
  let affected_rows = sqlx::query!(
    r#"
      INSERT INTO af_user (uuid, email, name)
      VALUES ($1, $2, $3)
      ON CONFLICT (email) DO NOTHING;
      "#,
    user_uuid,
    email,
    name
  )
  .execute(pool)
  .await
  .context(format!(
    "Fail to insert user with uuid: {}, name: {}, email: {}",
    user_uuid, name, email
  ))?
  .rows_affected();

  Ok(affected_rows > 0)
}

pub async fn uid_from_uuid(pool: &PgPool, gotrue_uuid: &uuid::Uuid) -> Result<i64, DatabaseError> {
  let uid = sqlx::query!(
    r#"
      SELECT uid FROM af_user WHERE uuid = $1
    "#,
    gotrue_uuid
  )
  .fetch_one(pool)
  .await?
  .uid;
  Ok(uid)
}
