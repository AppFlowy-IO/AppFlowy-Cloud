use std::sync::Arc;

use access_control::casbin::access::AccessControl;
use dashmap::DashMap;
use futures_util::StreamExt;
use sqlx::PgPool;
use uuid::Uuid;

use crate::collab::storage::CollabAccessControlStorage;
use crate::config::Config;
use crate::metrics::CollabMetrics;
use crate::pg_listener::PgListeners;
use crate::CollabRealtimeMetrics;
use access_control::metrics::AccessControlMetrics;
use app_error::AppError;
use collab_stream::stream_router::StreamRouter;
use database::user::{select_all_uid_uuid, select_uid_from_uuid};
use indexer::metrics::EmbeddingMetrics;
use indexer::scheduler::IndexerScheduler;

pub type RedisConnectionManager = redis::aio::ConnectionManager;

#[derive(Clone)]
pub struct AppState {
  pub config: Arc<Config>,
  pub pg_listeners: Arc<PgListeners>,
  pub user_cache: UserCache,
  pub redis_stream_router: Arc<StreamRouter>,
  pub redis_connection_manager: RedisConnectionManager,
  pub access_control: AccessControl,
  pub collab_access_control_storage: Arc<CollabAccessControlStorage>,
  pub metrics: AppMetrics,
  pub indexer_scheduler: Arc<IndexerScheduler>,
}

#[derive(Clone)]
pub struct AppMetrics {
  #[allow(dead_code)]
  pub registry: Arc<prometheus_client::registry::Registry>,
  pub access_control_metrics: Arc<AccessControlMetrics>,
  pub realtime_metrics: Arc<CollabRealtimeMetrics>,
  pub collab_metrics: Arc<CollabMetrics>,
  pub embedding_metrics: Arc<EmbeddingMetrics>,
}

impl Default for AppMetrics {
  fn default() -> Self {
    Self::new()
  }
}

impl AppMetrics {
  pub fn new() -> Self {
    let mut registry = prometheus_client::registry::Registry::default();
    let access_control_metrics = Arc::new(AccessControlMetrics::register(&mut registry));
    let realtime_metrics = Arc::new(CollabRealtimeMetrics::register(&mut registry));
    let collab_metrics = Arc::new(CollabMetrics::register(&mut registry));
    let embedding_metrics = Arc::new(EmbeddingMetrics::register(&mut registry));
    Self {
      registry: Arc::new(registry),
      access_control_metrics,
      realtime_metrics,
      collab_metrics,
      embedding_metrics,
    }
  }
}

pub struct AuthenticateUser {
  pub uid: i64,
}

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
