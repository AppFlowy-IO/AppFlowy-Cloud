use database_entity::dto::{AFAccessLevel, AFRole};

pub mod access_control;
pub mod adapter;

pub const MODEL_CONF: &str = r###"
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
"###;

/// Represents the entity stored at the index of the access control policy.
/// `user_id, object_id, role/action`
///
/// E.g. user1, collab::123, Owner
const POLICY_FIELD_INDEX_USER: usize = 0;
const POLICY_FIELD_INDEX_OBJECT: usize = 1;
const POLICY_FIELD_INDEX_ACTION: usize = 2;

/// Represents the entity stored at the index of the grouping.
/// `role, action`
///
/// E.g. Owner, Write
#[allow(dead_code)]
const GROUPING_FIELD_INDEX_ROLE: usize = 0;
#[allow(dead_code)]
const GROUPING_FIELD_INDEX_ACTION: usize = 1;

/// Represents the object type that is stored in the access control policy.
#[derive(Debug)]
enum ObjectType<'id> {
  /// Stored as `workspace::<uuid>`
  Workspace(&'id str),
  /// Stored as `collab::<uuid>`
  Collab(&'id str),
}

impl ToString for ObjectType<'_> {
  fn to_string(&self) -> String {
    match self {
      ObjectType::Collab(s) => format!("collab::{}", s),
      ObjectType::Workspace(s) => format!("workspace::{}", s),
    }
  }
}

/// Represents the action type that is stored in the access control policy.
#[derive(Debug)]
enum ActionType {
  Role(AFRole),
  Level(AFAccessLevel),
}

/// Represents the actions that can be performed on objects.
#[derive(Debug)]
enum Action {
  Read,
  Write,
  Delete,
}

impl ToString for Action {
  fn to_string(&self) -> String {
    match self {
      Action::Read => "read".to_owned(),
      Action::Write => "write".to_owned(),
      Action::Delete => "delete".to_owned(),
    }
  }
}

#[cfg(test)]
mod tests {
  use anyhow::Context;
  use lazy_static::lazy_static;
  use snowflake::Snowflake;
  use sqlx::PgPool;
  use tokio::sync::RwLock;
  use uuid::Uuid;

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
}
