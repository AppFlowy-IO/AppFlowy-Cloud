use crate::application::GoTrueAdmin;
use crate::biz::casbin::{CollabAccessControlImpl, WorkspaceAccessControlImpl};
use crate::biz::collab::storage::CollabPostgresDBStorage;
use crate::biz::pg_listener::PgListeners;

use crate::config::config::Config;
use app_error::AppError;

use crate::api::metrics::RequestMetrics;
use crate::biz::casbin::access_control::AccessControl;
use crate::biz::casbin::metrics::AccessControlMetrics;
use dashmap::DashMap;
use database::file::bucket_s3_impl::S3BucketStorage;
use database::user::{select_all_uid_uuid, select_uid_from_uuid};
use realtime::collaborate::RealtimeMetrics;
use snowflake::Snowflake;
use sqlx::PgPool;

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use uuid::Uuid;

pub type RedisClient = redis::aio::ConnectionManager;

#[derive(Clone)]
pub struct AppState {
  pub pg_pool: PgPool,
  pub config: Arc<Config>,
  pub users: Arc<UserCache>,
  pub id_gen: Arc<RwLock<Snowflake>>,
  pub gotrue_client: gotrue::api::Client,
  pub gotrue_admin: GoTrueAdmin,
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
  users: DashMap<Uuid, AuthenticateUser>,
}

impl UserCache {
  /// Load all users from database when initializing the cache.
  pub async fn new(pool: PgPool) -> Self {
    let users = {
      let users = DashMap::new();
      let mut stream = select_all_uid_uuid(&pool);
      while let Some(Ok(af_user_id)) = stream.next().await {
        users.insert(
          af_user_id.uuid,
          AuthenticateUser {
            uid: af_user_id.uid,
          },
        );
      }
      users
    };

    Self { pool, users }
  }

  /// Get the user's uid from the cache or the database.
  pub async fn get_user_uid(&self, uuid: &Uuid) -> Result<i64, AppError> {
    if let Some(entry) = self.users.get(uuid) {
      return Ok(entry.value().uid);
    }

    // If the user is not found in the cache, query the database.
    let uid = select_uid_from_uuid(&self.pool, uuid).await?;
    self.users.insert(*uuid, AuthenticateUser { uid });
    Ok(uid)
  }
}

#[derive(Clone)]
pub struct AppMetrics {
  #[allow(dead_code)]
  pub registry: Arc<prometheus_client::registry::Registry>,
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
