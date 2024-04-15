use anyhow::Context;
use collab_stream::client::CollabRedisStream;
use rand::{thread_rng, Rng};
use sqlx::PgPool;

pub async fn redis_client() -> redis::Client {
  let redis_uri = "redis://localhost:6379";
  redis::Client::open(redis_uri)
    .context("failed to connect to redis")
    .unwrap()
}

pub async fn redis_stream() -> CollabRedisStream {
  let redis_client = redis_client().await;
  CollabRedisStream::new(redis_client)
    .await
    .context("failed to create stream client")
    .unwrap()
}

#[allow(dead_code)]
pub fn random_i64() -> i64 {
  let mut rng = thread_rng();
  let num: i64 = rng.gen();
  num
}

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

  sqlx::migrate!("../../migrations")
    .set_ignore_missing(true)
    .run(pool)
    .await
    .unwrap();
  Ok(())
}
