use crate::api;
use crate::config::{Config, DatabaseSetting};
use crate::core::manager::OpenCollabManager;
use anyhow::Error;
use axum::http::Method;
use collab_stream::client::CollabRedisStream;
use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::api::HistoryImpl;
use std::sync::Arc;
use std::time::Duration;
use tonic::transport::server::{Router, Routes, RoutesBuilder};
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tonic_proto::history::history_server::{History, HistoryServer};
use tonic_proto::history::{RepeatedSnapshotMeta, SnapshotRequest};
use tower::layer::util::Identity;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

pub async fn create_app(config: Config) -> Result<Router<Identity>, Error> {
  // Postgres
  info!("Preparing to run database migrations...");
  let pg_pool = get_connection_pool(&config.db_settings).await?;
  migrate(&pg_pool).await?;

  // Redis
  let redis_client = redis::Client::open(config.redis_url)
    .expect("failed to create redis client")
    .get_connection_manager()
    .await
    .expect("failed to get redis connection manager");

  let collab_redis_stream =
    CollabRedisStream::new_with_connection_manager(redis_client.clone()).await;
  let open_collab_manager =
    Arc::new(OpenCollabManager::new(collab_redis_stream, pg_pool.clone()).await);

  let state = AppState {
    redis_client,
    open_collab_manager,
    pg_pool,
  };

  let history_impl = HistoryImpl { state };
  Ok(Server::builder().add_service(HistoryServer::new(history_impl)))
}

#[derive(Clone)]
pub struct AppState {
  pub redis_client: ConnectionManager,
  pub open_collab_manager: Arc<OpenCollabManager>,
  pub pg_pool: PgPool,
}

async fn migrate(pool: &PgPool) -> Result<(), Error> {
  sqlx::migrate!("../../migrations")
    .set_ignore_missing(true)
    .run(pool)
    .await
    .map_err(|e| anyhow::anyhow!("Failed to run migrations: {}", e))
}

async fn get_connection_pool(setting: &DatabaseSetting) -> Result<PgPool, Error> {
  info!(
    "Connecting to postgres database with setting: {:?}",
    setting
  );
  PgPoolOptions::new()
    .max_connections(setting.max_connections)
    .acquire_timeout(Duration::from_secs(10))
    .max_lifetime(Duration::from_secs(30 * 60))
    .idle_timeout(Duration::from_secs(30))
    .connect_with(setting.with_db())
    .await
    .map_err(|e| anyhow::anyhow!("Failed to connect to postgres database: {}", e))
}
