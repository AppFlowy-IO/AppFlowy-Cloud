use crate::api;
use crate::config::{Config, DatabaseSetting};
use crate::core::manager::OpenCollabManager;
use anyhow::Error;
use axum::http::Method;
use axum::Router;
use collab_stream::client::CollabRedisStream;
use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

pub async fn create_app(config: Config) -> Result<Router<()>, Error> {
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

  let cors = CorsLayer::new()
        // allow `GET` and `POST` when accessing the resource
        .allow_methods([Method::GET, Method::POST])
        // allow requests from any origin
        .allow_origin(Any);
  Ok(
    Router::new()
      .nest_service("/api", api::router().with_state(state.clone()))
      .layer(ServiceBuilder::new().layer(cors)),
  )
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
