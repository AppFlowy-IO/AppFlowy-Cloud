use crate::config::{Config, DatabaseSetting, Environment};
use anyhow::Error;
use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::notion_import::task_queue::run_notion_importer;
use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use std::sync::Once;
use std::time::Duration;
use tokio::net::TcpListener;

use tracing::info;
use tracing::subscriber::set_global_default;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

pub async fn run_server(
  listener: TcpListener,
  config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
  dotenvy::dotenv().ok();
  init_subscriber(&config.app_env);
  info!("config loaded: {:?}", &config);

  // Start the server
  info!("Starting server at: {:?}", listener.local_addr());
  let app = create_app(config).await.unwrap();
  axum::serve(listener, app).await?;
  Ok(())
}

pub fn init_subscriber(app_env: &Environment) {
  static START: Once = Once::new();
  START.call_once(|| {
    let level = std::env::var("RUST_LOG").unwrap_or("info".to_string());
    let mut filters = vec![];
    filters.push(format!("appflowy_worker={}", level));
    let env_filter = EnvFilter::new(filters.join(","));

    let builder = tracing_subscriber::fmt()
      .with_target(true)
      .with_max_level(tracing::Level::TRACE)
      .with_thread_ids(false)
      .with_file(false);

    match app_env {
      Environment::Local => {
        let subscriber = builder
          .with_ansi(true)
          .with_target(false)
          .with_file(false)
          .pretty()
          .finish()
          .with(env_filter);
        set_global_default(subscriber).unwrap();
      },
      Environment::Production => {
        let subscriber = builder.json().finish().with(env_filter);
        set_global_default(subscriber).unwrap();
      },
    }
  });
}

pub async fn create_app(config: Config) -> Result<Router<()>, Error> {
  // Postgres
  info!("Preparing to run database migrations...");
  let pg_pool = get_connection_pool(&config.db_settings).await?;

  // Redis
  let redis_client = redis::Client::open(config.redis_url)
    .expect("failed to create redis client")
    .get_connection_manager()
    .await
    .expect("failed to get redis connection manager");

  let state = AppState {
    redis_client,
    pg_pool,
  };

  run_notion_importer(state.redis_client.clone());

  let app = Router::new()
    .route("/health", get(health_check))
    .with_state(state);

  Ok(app)
}

async fn health_check() -> StatusCode {
  StatusCode::OK
}

#[derive(Clone)]
pub struct AppState {
  pub redis_client: ConnectionManager,
  pub pg_pool: PgPool,
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
