use anyhow::Context;

use lazy_static::lazy_static;
use snowflake::Snowflake;
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

mod collab_ac_test;
mod member_ac_test;
mod user_ac_test;

pub const MODEL_CONF: &str = r#"
[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[role_definition]
g = _, _ # role to action
g2 = _, _ # worksheet to collab

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = r.sub == p.sub && g2(p.obj, r.obj) && g(p.act, r.act)
"#;

lazy_static! {
  pub static ref ID_GEN: RwLock<Snowflake> = RwLock::new(Snowflake::new(1));
}

pub async fn setup_db(pool: &PgPool) -> anyhow::Result<()> {
  // Have to manually manage schema and tables managed by gotrue but referenced by our
  // migration scripts.

  // Create schema and tables
  sqlx::query(r#"create schema auth"#).execute(pool).await?;
  sqlx::query(
    r#"create table auth.users(
id uuid NOT NULL UNIQUE,
deleted_at timestamptz null,
CONSTRAINT users_pkey PRIMARY KEY (id)
)"#,
  )
  .execute(pool)
  .await?;

  // Manually run migration after creating required objects above.
  sqlx::migrate!().run(pool).await?;

  // Remove foreign key constraint
  sqlx::query(r#"alter table public.af_user drop constraint af_user_email_foreign_key"#)
    .execute(pool)
    .await?;

  Ok(())
}

#[derive(Debug, Clone)]
pub struct User {
  pub uid: i64,
  pub uuid: Uuid,
  pub email: String,
}

pub async fn create_user(pool: &PgPool) -> anyhow::Result<User> {
  // Create user and workspace
  let uid = ID_GEN.write().await.next_id();
  let uuid = Uuid::new_v4();
  let email = format!("{}@appflowy.io", uuid);
  let name = uuid.to_string();
  database::user::create_user(pool, uid, &uuid, &email, &name)
    .await
    .context("create user")?;

  Ok(User { uid, uuid, email })
}
