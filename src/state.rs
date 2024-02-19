use crate::biz::casbin::{CollabAccessControlImpl, WorkspaceAccessControlImpl};
use crate::biz::collab::storage::CollabPostgresDBStorage;
use crate::biz::pg_listener::PgListeners;

use crate::config::config::Config;
use app_error::AppError;

use crate::api::metrics::RequestMetrics;
use crate::biz::casbin::access_control::AccessControl;
use crate::biz::casbin::metrics::AccessControlMetrics;
use database::file::bucket_s3_impl::S3BucketStorage;
use database::user::select_uid_from_uuid;
use realtime::collaborate::RealtimeMetrics;
use snowflake::Snowflake;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub type RedisClient = redis::aio::ConnectionManager;

#[derive(Clone)]
pub struct AppState {
  pub pg_pool: PgPool,
  pub config: Arc<Config>,
  pub users: Arc<UserCache>,
  pub id_gen: Arc<RwLock<Snowflake>>,
  pub gotrue_client: gotrue::api::Client,
  pub redis_client: RedisClient,
  pub collab_storage: Arc<CollabPostgresDBStorage>,
  pub collab_access_control: CollabAccessControlImpl,
  pub workspace_access_control: WorkspaceAccessControlImpl,
  pub bucket_storage: Arc<S3BucketStorage>,
  pub pg_listeners: Arc<PgListeners>,
  pub access_control: AccessControl,
  pub metrics: AppMetrics,
}

impl AppState {
  pub async fn load_users(_pool: &PgPool) {
    todo!()
  }

  pub async fn next_user_id(&self) -> i64 {
    self.id_gen.write().await.next_id()
  }
}

pub struct AuthenticateUser {
  pub uid: i64,
}

pub const EXPIRED_DURATION_DAYS: i64 = 30;

pub struct UserCache {
  pool: PgPool,
  users: RwLock<HashMap<Uuid, AuthenticateUser>>,
}

impl UserCache {
  pub fn new(pool: PgPool) -> Self {
    Self {
      pool,
      users: RwLock::new(HashMap::new()),
    }
  }

  /// Get the user's uid from the cache or the database.
  pub async fn get_user_uid(&self, uuid: &Uuid) -> Result<i64, AppError> {
    // Attempt to acquire a read lock and check if the user exists to minimize lock contention.
    {
      let users_read = self.users.read().await;
      if let Some(user) = users_read.get(uuid) {
        return Ok(user.uid);
      }
    }

    // If the user is not found in the cache, query the database.
    let uid = select_uid_from_uuid(&self.pool, uuid).await?;
    let mut users_write = self.users.write().await;
    users_write.insert(*uuid, AuthenticateUser { uid });
    Ok(uid)
  }
}

#[derive(Clone)]
pub struct AppMetrics {
  #[allow(dead_code)]
  registry: Arc<prometheus_client::registry::Registry>,
  pub request_metrics: Arc<RequestMetrics>,
  pub realtime_metrics: Arc<RealtimeMetrics>,
  pub access_control_metrics: Arc<AccessControlMetrics>,
}

impl Default for AppMetrics {
  fn default() -> Self {
    Self::new()
  }
}

impl AppMetrics {
  pub fn new() -> Self {
    let mut registry = prometheus_client::registry::Registry::default();
    let request_metrics = Arc::new(RequestMetrics::register(&mut registry));
    let realtime_metrics = Arc::new(RealtimeMetrics::register(&mut registry));
    let access_control_metrics = Arc::new(AccessControlMetrics::register(&mut registry));
    Self {
      registry: Arc::new(registry),
      request_metrics,
      realtime_metrics,
      access_control_metrics,
    }
  }
}
