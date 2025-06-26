use access_control::collab::{CollabAccessControl, RealtimeAccessControl};
use access_control::workspace::WorkspaceAccessControl;
use actix::Addr;
use anyhow::anyhow;
use dashmap::DashMap;
use gotrue_entity::gotrue_jwt::GoTrueServiceRoleClaims;
use secrecy::{ExposeSecret, Secret};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use uuid::Uuid;

use access_control::metrics::AccessControlMetrics;
use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
use appflowy_collaborate::collab::cache::CollabCache;
use appflowy_collaborate::metrics::CollabMetrics;
use appflowy_collaborate::ws2::WsServer;
use appflowy_collaborate::CollabRealtimeMetrics;
use collab_stream::awareness_gossip::AwarenessGossip;
use collab_stream::metrics::CollabStreamMetrics;
use collab_stream::stream_router::StreamRouter;
use database::collab::CollabStore;
use database::file::s3_client_impl::{AwsS3BucketClientImpl, S3BucketStorage};
use database::user::{select_all_uid_uuid, select_uid_from_uuid};
use indexer::metrics::EmbeddingMetrics;
use indexer::scheduler::IndexerScheduler;
use snowflake::Snowflake;

use crate::api::metrics::{AppFlowyWebMetrics, PublishedCollabMetrics, RequestMetrics};
use crate::biz::chat::metrics::AIMetrics;
use crate::biz::pg_listener::PgListeners;
use crate::biz::workspace::publish::PublishedCollabStore;
use crate::config::config::Config;
use crate::mailer::AFCloudMailer;

pub type RedisConnectionManager = redis::aio::ConnectionManager;
#[derive(Clone)]
pub struct AppState {
  pub pg_pool: PgPool,
  pub config: Arc<Config>,
  pub user_cache: UserCache,
  pub id_gen: Arc<RwLock<Snowflake>>,
  pub gotrue_client: gotrue::api::Client,
  pub redis_stream_router: Arc<StreamRouter>,
  pub awareness_gossip: Arc<AwarenessGossip>,
  pub redis_connection_manager: RedisConnectionManager,
  pub collab_cache: Arc<CollabCache>,
  pub collab_storage: Arc<dyn CollabStore>,
  pub collab_access_control: Arc<dyn CollabAccessControl>,
  pub workspace_access_control: Arc<dyn WorkspaceAccessControl>,
  pub realtime_access_control: Arc<dyn RealtimeAccessControl>,
  pub bucket_storage: Arc<S3BucketStorage>,
  pub published_collab_store: Arc<dyn PublishedCollabStore>,
  pub bucket_client: AwsS3BucketClientImpl,
  pub pg_listeners: Arc<PgListeners>,
  pub metrics: AppMetrics,
  pub gotrue_admin: GoTrueAdmin,
  pub mailer: AFCloudMailer,
  pub ai_client: AppFlowyAIClient,
  pub indexer_scheduler: Arc<IndexerScheduler>,
  pub ws_server: Addr<WsServer>,
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
  pub published_collab_metrics: Arc<PublishedCollabMetrics>,
  pub appflowy_web_metrics: Arc<AppFlowyWebMetrics>,
  pub embedding_metrics: Arc<EmbeddingMetrics>,
  pub collab_stream_metrics: Arc<CollabStreamMetrics>,
  pub ai_metrics: Arc<AIMetrics>,
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
    let published_collab_metrics = Arc::new(PublishedCollabMetrics::register(&mut registry));
    let appflowy_web_metrics = Arc::new(AppFlowyWebMetrics::register(&mut registry));
    let embedding_metrics = Arc::new(EmbeddingMetrics::register(&mut registry));
    let collab_stream_metrics = Arc::new(CollabStreamMetrics::register(&mut registry));
    let ai_metrics = Arc::new(AIMetrics::register(&mut registry));
    Self {
      registry: Arc::new(registry),
      request_metrics,
      realtime_metrics,
      access_control_metrics,
      collab_metrics,
      published_collab_metrics,
      appflowy_web_metrics,
      embedding_metrics,
      collab_stream_metrics,
      ai_metrics,
    }
  }
}

#[derive(Debug, Clone)]
pub struct GoTrueAdmin {
  pub gotrue_client: gotrue::api::Client,
  pub jwt_secret: Secret<String>,
  pub service_role: String,
}

impl GoTrueAdmin {
  pub fn new(jwt_secret: String, service_role: String, gotrue_client: gotrue::api::Client) -> Self {
    Self {
      jwt_secret: jwt_secret.into(),
      gotrue_client,
      service_role,
    }
  }

  pub async fn token(&self) -> Result<String, AppError> {
    let claims = GoTrueServiceRoleClaims {
      role: self.service_role.clone(),
    };
    claims
      .encode(self.jwt_secret.expose_secret().as_bytes())
      .map_err(|err| AppError::Internal(anyhow!(err.to_string())))
  }
}
