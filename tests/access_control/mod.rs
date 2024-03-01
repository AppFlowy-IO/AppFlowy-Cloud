use actix_http::Method;
use anyhow::{Context, Error};
use app_error::{AppError, ErrorCode};
use appflowy_cloud::biz::casbin::{
  AFEnforcerCache, ActionCacheKey, CollabAccessControlImpl, PolicyCacheKey,
  WorkspaceAccessControlImpl,
};
use appflowy_cloud::biz::workspace::access_control::WorkspaceAccessControl;
use client_api_test_util::setup_log;
use database_entity::dto::{AFAccessLevel, AFRole};
use lazy_static::lazy_static;
use realtime::collaborate::CollabAccessControl;
use snowflake::Snowflake;
use sqlx::PgPool;
use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{interval, timeout};

use appflowy_cloud::biz::casbin::access_control::AccessControl;

use appflowy_cloud::biz::pg_listener::PgListeners;
use appflowy_cloud::state::AppMetrics;
use uuid::Uuid;

mod collab_ac_test;
mod member_ac_test;
mod user_ac_test;

lazy_static! {
  pub static ref ID_GEN: RwLock<Snowflake> = RwLock::new(Snowflake::new(1));
}

pub async fn setup_access_control(pool: &PgPool) -> anyhow::Result<AccessControl> {
  setup_db(pool).await?;

  let metrics = AppMetrics::new();
  let listeners = PgListeners::new(pool).await?;
  let enforcer_cache = Arc::new(TestEnforcerCacheImpl {
    cache: DashMap::new(),
  });
  Ok(
    AccessControl::new(
      pool.clone(),
      listeners.subscribe_collab_member_change(),
      listeners.subscribe_workspace_member_change(),
      metrics.access_control_metrics,
      enforcer_cache,
    )
    .await
    .unwrap(),
  )
}

pub async fn setup_db(pool: &PgPool) -> anyhow::Result<()> {
  setup_log();
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
  access_control: &CollabAccessControlImpl,
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
           uid,
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
  access_control: &WorkspaceAccessControlImpl,
  uid: &i64,
  workspace_id: &Uuid,
  expected_role: Option<AFRole>,
  pg_pool: &PgPool,
) {
  let mut retry_count = 0;
  let timeout = Duration::from_secs(10);
  let start_time = tokio::time::Instant::now();

  loop {
    if retry_count > 10 {
      // This check should be outside of the select! block to prevent panic before checking the condition.
      panic!("Exceeded maximum number of retries");
    }

    if start_time.elapsed() > timeout {
      panic!("can't get the expected role before timeout");
    }

    match access_control
      .get_workspace_role(uid, workspace_id, pg_pool)
      .await
    {
      Ok(role) if Some(&role) == expected_role.as_ref() => {
        // If the roles match, or if the expected role is None and no role is found, break the loop
        break;
      },
      Err(err) if err.is_record_not_found() && expected_role.is_none() => {
        // If no record is found and no role is expected, break the loop
        break;
      },
      Err(err) if err.is_record_not_found() => {
        // If no record is found but a role is expected, wait and retry
        tokio::time::sleep(Duration::from_millis(1000)).await;
      },
      _ => {
        // If the roles do not match, or any other error occurs, wait and retry
        tokio::time::sleep(Duration::from_millis(300)).await;
      },
    }

    retry_count += 1;
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
  access_control: &WorkspaceAccessControlImpl,
  uid: &i64,
  workspace_id: &Uuid,
  expected_error: ErrorCode,
  pg_pool: &PgPool,
) {
  let timeout_duration = Duration::from_secs(10);
  let retry_interval = Duration::from_millis(300);
  let mut retries = 0usize;
  let max_retries = 10;

  let operation = async {
    let mut interval = interval(retry_interval);
    loop {
      interval.tick().await; // Wait for the next interval tick before retrying
      match access_control
        .get_workspace_role(uid, workspace_id, pg_pool)
        .await
      {
        Ok(_) => {},
        Err(err) if err.code() == expected_error => {
          // If the error matches the expected error, exit successfully
          return;
        },
        Err(_) => {
          retries += 1;
          if retries > max_retries {
            // If retries exceed the maximum, return an error
            panic!("Exceeded maximum number of retries without encountering the expected error");
          }
          // On any other error, continue retrying
        },
      }
    }
  };

  timeout(timeout_duration, operation)
    .await
    .expect("Operation timed out");
}

pub async fn assert_can_access_http_method(
  access_control: &CollabAccessControlImpl,
  uid: &i64,
  object_id: &str,
  method: Method,
  expected: bool,
) -> Result<(), Error> {
  let timeout_duration = Duration::from_secs(10);
  let retry_interval = Duration::from_millis(1000);
  let mut retries = 0usize;
  let max_retries = 10;

  let operation = async {
    let mut interval = interval(retry_interval);
    loop {
      interval.tick().await; // Wait for the next interval tick before retrying
      match access_control
        .can_access_http_method(uid, object_id, &method)
        .await
      {
        Ok(access) => {
          if access == expected {
            break;
          }
        },
        Err(_) => {
          retries += 1;
          if retries > max_retries {
            // If retries exceed the maximum, return an error
            panic!("Exceeded maximum number of retries without encountering the expected error");
          }
          // On any other error, continue retrying
        },
      }
    }
  };

  timeout(timeout_duration, operation).await?;
  Ok(())
}

struct TestEnforcerCacheImpl {
  cache: DashMap<String, String>,
}

#[async_trait]
impl AFEnforcerCache for TestEnforcerCacheImpl {
  async fn set_enforcer_result(&self, key: &PolicyCacheKey, value: bool) -> Result<(), AppError> {
    self
      .cache
      .insert(key.as_ref().to_string(), value.to_string());
    Ok(())
  }

  async fn get_enforcer_result(&self, key: &PolicyCacheKey) -> Option<bool> {
    self
      .cache
      .get(key.as_ref())
      .map(|v| v.value().parse().unwrap())
  }

  async fn remove_enforcer_result(&self, key: &PolicyCacheKey) {
    self.cache.remove(key.as_ref());
  }

  async fn set_action(&self, key: &ActionCacheKey, value: String) -> Result<(), AppError> {
    self.cache.insert(key.as_ref().to_string(), value);
    Ok(())
  }

  async fn get_action(&self, key: &ActionCacheKey) -> Option<String> {
    self.cache.get(key.as_ref()).map(|v| v.value().to_string())
  }

  async fn remove_action(&self, key: &ActionCacheKey) {
    self.cache.remove(key.as_ref());
  }
}
