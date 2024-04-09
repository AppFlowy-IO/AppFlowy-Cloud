use lazy_static::lazy_static;
use snowflake::Snowflake;
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

pub async fn setup_db(pool: &PgPool) -> anyhow::Result<()> {
  // Have to manually create schema and tables managed by gotrue but referenced by our
  // migration scripts.
  sqlx::query(r#"create schema auth"#).execute(pool).await?;
  sqlx::query(
    r#"
      CREATE TABLE auth.users(
        id uuid NOT NULL UNIQUE,
        deleted_at timestamptz null,
        CONSTRAINT users_pkey PRIMARY KEY (id)
      )
    "#,
  )
  .execute(pool)
  .await?;

  sqlx::migrate!("./migrations")
    .set_ignore_missing(true)
    .run(pool)
    .await
    .unwrap();
  Ok(())
}

pub async fn insert_auth_user(pool: &PgPool, user_uuid: Uuid) -> anyhow::Result<()> {
  sqlx::query(
    r#"
      INSERT INTO auth.users (id)
      VALUES ($1)
    "#,
  )
  .bind(user_uuid)
  .execute(pool)
  .await?;
  Ok(())
}

lazy_static! {
  pub static ref ID_GEN: RwLock<Snowflake> = RwLock::new(Snowflake::new(1));
}

pub async fn test_create_user(
  pool: &PgPool,
  user_uuid: Uuid,
  email: &str,
  name: &str,
) -> anyhow::Result<()> {
  let uid = ID_GEN.write().await.next_id();
  database::user::create_user(pool, uid, &user_uuid, email, name)
    .await
    .unwrap();
  Ok(())
}
