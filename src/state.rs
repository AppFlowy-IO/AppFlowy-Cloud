use std::sync::Arc;

use appflowy_ai_client::client::AppFlowyAIClient;
use dashmap::DashMap;
use secrecy::{ExposeSecret, Secret};
use sqlx::PgPool;
use tokio::sync::{Mutex, RwLock};
use tokio_stream::StreamExt;
use uuid::Uuid;

use access_control::access::AccessControl;
use access_control::metrics::AccessControlMetrics;
use app_error::AppError;
use appflowy_collaborate::collab::access_control::CollabAccessControlImpl;
use appflowy_collaborate::collab::cache::CollabCache;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use appflowy_collaborate::metrics::CollabMetrics;
use appflowy_collaborate::shared_state::RealtimeSharedState;
use appflowy_collaborate::CollabRealtimeMetrics;
use database::file::bucket_s3_impl::S3BucketStorage;
use database::user::{select_all_uid_uuid, select_uid_from_uuid};
use gotrue::grant::{Grant, PasswordGrant};
use snowflake::Snowflake;
use tonic_proto::history::history_client::HistoryClient;
use workspace_access::WorkspaceAccessControlImpl;

use crate::api::metrics::RequestMetrics;
use crate::biz::pg_listener::PgListeners;
use crate::config::config::Config;
use crate::mailer::Mailer;

pub type RedisConnectionManager = redis::aio::ConnectionManager;
#[derive(Clone)]
pub struct AppState {
  pub pg_pool: PgPool,
  pub config: Arc<Config>,
  pub user_cache: UserCache,
  pub id_gen: Arc<RwLock<Snowflake>>,
  pub gotrue_client: gotrue::api::Client,
  pub redis_connection_manager: RedisConnectionManager,
  pub collab_cache: CollabCache,
  pub collab_access_control_storage: Arc<CollabAccessControlStorage>,
  pub collab_access_control: CollabAccessControlImpl,
  pub workspace_access_control: WorkspaceAccessControlImpl,
  pub bucket_storage: Arc<S3BucketStorage>,
  pub pg_listeners: Arc<PgListeners>,
  pub access_control: AccessControl,
  pub metrics: AppMetrics,
  pub gotrue_admin: GoTrueAdmin,
  pub mailer: Mailer,
  pub openai: Option<openai_dive::v1::api::Client>,
  pub ai_client: AppFlowyAIClient,
  pub realtime_shared_state: RealtimeSharedState,
  pub grpc_history_client: Arc<Mutex<HistoryClient<tonic::transport::Channel>>>,
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

#[derive(Clone)]
pub struct UserCache {
  pool: PgPool,
  users: Arc<DashMap<Uuid, AuthenticateUser>>,
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

    Self {
      pool,
      users: Arc::new(users),
    }
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
  pub realtime_metrics: Arc<CollabRealtimeMetrics>,
  pub access_control_metrics: Arc<AccessControlMetrics>,
  pub collab_metrics: Arc<CollabMetrics>,
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
    let realtime_metrics = Arc::new(CollabRealtimeMetrics::register(&mut registry));
    let access_control_metrics = Arc::new(AccessControlMetrics::register(&mut registry));
    let collab_metrics = Arc::new(CollabMetrics::register(&mut registry));
    Self {
      registry: Arc::new(registry),
      request_metrics,
      realtime_metrics,
      access_control_metrics,
      collab_metrics,
    }
  }
}

#[derive(Debug, Clone)]
pub struct GoTrueAdmin {
  pub admin_email: String,
  pub password: Secret<String>,
}

impl GoTrueAdmin {
  pub fn new(admin_email: String, password: String) -> Self {
    Self {
      admin_email,
      password: password.into(),
    }
  }

  pub async fn token(&self, client: &gotrue::api::Client) -> Result<String, AppError> {
    let token = client
      .token(&Grant::Password(PasswordGrant {
        email: self.admin_email.clone(),
        password: self.password.expose_secret().clone(),
      }))
      .await?;
    Ok(token.access_token)
  }
}
