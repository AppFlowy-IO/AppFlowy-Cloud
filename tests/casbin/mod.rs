use actix_http::Method;
use anyhow::Context;
use app_error::ErrorCode;
use appflowy_cloud::biz::casbin::access_control::{
  CasbinCollabAccessControl, CasbinWorkspaceAccessControl,
};
use appflowy_cloud::biz::workspace::access_control::WorkspaceAccessControl;
use database_entity::dto::{AFAccessLevel, AFRole};
use lazy_static::lazy_static;
use realtime::collaborate::{CollabAccessControl, CollabUserId};
use snowflake::Snowflake;
use sqlx::PgPool;
use std::time::Duration;
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

/// Asserts that the user has the specified access level within a workspace.
///
/// This function continuously checks the user's access level in a workspace and asserts that it
/// matches the expected level. The function retries the check a fixed number of times before timing out.
///
/// # Panics
/// Panics if the expected access level is not achieved before the timeout.
pub async fn assert_access_level<T: AsRef<str>>(
  access_control: &CasbinCollabAccessControl,
  uid: &i64,
  workspace_id: T,
  expected_level: Option<AFAccessLevel>,
) {
  let mut retry_count = 0;
  loop {
    tokio::select! {
       _ = tokio::time::sleep(Duration::from_secs(10)) => {
         panic!("can't get the expected access level before timeout");
       },
       result = access_control
         .get_collab_access_level(
           CollabUserId::UserId(uid),
           workspace_id.as_ref(),
         )
       => {
        retry_count += 1;
        match result {
          Ok(access_level) => {
            if retry_count > 10 {
              assert_eq!(access_level, expected_level.unwrap());
              break;
            }
            if let Some(expected_level) = expected_level {
              if access_level == expected_level {
                break;
              }
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
          },
          Err(err) => {
            if err.is_record_not_found() & expected_level.is_none() {
              break;
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
          }
        }
       },
    }
  }
}
/// Asserts that the user has the specified role within a workspace.
///
/// This function continuously checks the user's role in a workspace and asserts that it matches
/// the expected role. It retries the check a fixed number of times before timing out.
///
/// # Panics
/// Panics if the expected role is not achieved before the timeout.

pub async fn assert_workspace_role(
  access_control: &CasbinWorkspaceAccessControl,
  uid: &i64,
  workspace_id: &Uuid,
  expected_role: Option<AFRole>,
) {
  let mut retry_count = 0;
  loop {
    tokio::select! {
       _ = tokio::time::sleep(Duration::from_secs(10)) => {
         panic!("can't get the expected role before timeout");
       },
       result = access_control
         .get_role_from_uid(uid, workspace_id)
       => {
        retry_count += 1;
        match result {
          Ok(role) => {
            if retry_count > 10 {
              assert_eq!(role, expected_role.unwrap());
              break;
            }
            if let Some(expected_role) = &expected_role {
              if &role == expected_role {
                break;
              }
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
          },
          Err(err) => {
            if err.is_record_not_found() & expected_role.is_none() {
              break;
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
          }
        }
       },
    }
  }
}
/// Asserts that retrieving the user's role within a workspace results in a specific error.
///
/// This function continuously attempts to fetch the user's role in a workspace, expecting a specific
/// error. If the expected error does not occur within a certain number of retries, it panics.
///
/// # Panics
/// Panics if the expected error is not encountered before the timeout or if an unexpected role is received.

pub async fn assert_workspace_role_error(
  access_control: &CasbinWorkspaceAccessControl,
  uid: &i64,
  workspace_id: &Uuid,
  expected_error: ErrorCode,
) {
  let mut retry_count = 0;
  loop {
    tokio::select! {
       _ = tokio::time::sleep(Duration::from_secs(10)) => {
         panic!("can't get the expected role before timeout");
       },
       result = access_control
         .get_role_from_uid(uid, workspace_id)
       => {
        retry_count += 1;
        match result {
          Ok(role) => {
            if retry_count > 10 {
              panic!("expected error: {:?}, but got role: {:?}", expected_error, role);
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
          },
          Err(err) => {
            if err.code() == expected_error {
              break;
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
          }
        }
       },
    }
  }
}
/// Asserts whether the user has access to a specific HTTP method on a given object.
///
/// This function continuously checks if the user is allowed to access a particular HTTP method
/// on an object and asserts that the result matches the expected outcome. It retries the check
/// a fixed number of times before timing out.
///
/// # Arguments
/// * `access_control` - A reference to the `CasbinCollabAccessControl` instance.
/// * `uid` - The user ID for which to check the access.
/// * `object_id` - The ID of the object being accessed.
/// * `method` - The HTTP method (`Method`) to check.
/// * `expected` - The expected boolean result of the access check.
///
/// # Panics
/// Panics if the expected access result is not achieved before the timeout.

pub async fn assert_can_access_http_method(
  access_control: &CasbinCollabAccessControl,
  uid: &i64,
  object_id: &str,
  method: Method,
  expected: bool,
) {
  let mut retry_count = 0;
  loop {
    tokio::select! {
       _ = tokio::time::sleep(Duration::from_secs(10)) => {
         panic!("can't get the expected access level before timeout");
       },
       result = access_control
         .can_access_http_method(
           CollabUserId::UserId(uid),
           object_id,
           &method,
         )
       => {
        retry_count += 1;
        match result {
          Ok(access) => {
            if retry_count > 10 {
              assert_eq!(access, expected);
              break;
            }
            if access == expected {
              break;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
          },
          Err(_err) => {
            tokio::time::sleep(Duration::from_millis(1000)).await;
          }
        }
       },
    }
  }
}
