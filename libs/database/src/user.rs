use sqlx::PgPool;

pub async fn update_user_name(
  pool: &PgPool,
  uuid: &uuid::Uuid,
  name: &str,
) -> Result<(), sqlx::Error> {
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

pub async fn create_user_if_not_exists(
  pool: &PgPool,
  user_uuid: &uuid::Uuid,
  email: &str,
  name: &str,
) -> Result<bool, sqlx::Error> {
  let affected_rows = sqlx::query!(
    r#"
      INSERT INTO af_user (uuid, email, name)
      SELECT $1, $2, $3
      WHERE NOT EXISTS (
          SELECT 1 FROM public.af_user WHERE email = $2
      )
      AND NOT EXISTS (
          SELECT 1 FROM public.af_user WHERE uuid = $1
      )
      "#,
    user_uuid,
    email,
    name
  )
  .execute(pool)
  .await?
  .rows_affected();

  Ok(affected_rows > 0)
}

pub async fn get_user_id(pool: &PgPool, gotrue_uuid: &uuid::Uuid) -> Result<i64, sqlx::Error> {
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
