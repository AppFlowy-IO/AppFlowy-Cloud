use crate::biz::casbin::access_control::{
  CasbinAccessControl, CasbinCollabAccessControl, CasbinWorkspaceAccessControl,
};
use crate::biz::collab::storage::CollabPostgresDBStorage;
use crate::biz::pg_listener::PgListeners;
use crate::component::auth::LoggedUser;
use crate::config::config::Config;
use chrono::{DateTime, Utc};
use database::file::bucket_s3_impl::S3BucketStorage;
use snowflake::Snowflake;
use sqlx::PgPool;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
  pub pg_pool: PgPool,
  pub config: Arc<Config>,
  pub user: Arc<RwLock<UserCache>>,
  pub id_gen: Arc<RwLock<Snowflake>>,
  pub gotrue_client: gotrue::api::Client,
  pub redis_client: redis::aio::ConnectionManager,
  pub collab_storage: Arc<CollabPostgresDBStorage>,
  pub collab_access_control: CasbinCollabAccessControl,
  pub workspace_access_control: CasbinWorkspaceAccessControl,
  pub bucket_storage: Arc<S3BucketStorage>,
  pub pg_listeners: Arc<PgListeners>,
  pub casbin_access_control: CasbinAccessControl,
}

impl AppState {
  pub async fn load_users(_pool: &PgPool) {
    todo!()
  }

  pub async fn next_user_id(&self) -> i64 {
    self.id_gen.write().await.next_id()
  }
}

#[derive(Clone, Debug, Copy)]
enum AuthStatus {
  Authorized(DateTime<Utc>),
  NotAuthorized,
}

pub const EXPIRED_DURATION_DAYS: i64 = 30;

#[derive(Debug, Default)]
pub struct UserCache {
  // Keep track the user authentication state
  user: BTreeMap<i64, AuthStatus>,
}

impl UserCache {
  pub fn new() -> Self {
    UserCache::default()
  }

  pub fn is_authorized(&self, user: &LoggedUser) -> bool {
    match self.user.get(user.expose_secret()) {
      None => {
        tracing::debug!("user not login yet or server was reboot");
        false
      },
      Some(status) => match *status {
        AuthStatus::Authorized(last_time) => {
          let current_time = Utc::now();
          let days = (current_time - last_time).num_days();
          days < EXPIRED_DURATION_DAYS
        },
        AuthStatus::NotAuthorized => {
          tracing::debug!("user logout already");
          false
        },
      },
    }
  }

  pub fn authorized(&mut self, user: LoggedUser) {
    self.user.insert(
      user.expose_secret().to_owned(),
      AuthStatus::Authorized(Utc::now()),
    );
  }

  pub fn unauthorized(&mut self, user: LoggedUser) {
    self
      .user
      .insert(user.expose_secret().to_owned(), AuthStatus::NotAuthorized);
  }
}
