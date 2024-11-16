use crate::api::HistoryImpl;
use crate::config::{Config, DatabaseSetting, Environment};
use crate::core::manager::OpenCollabManager;
use anyhow::Error;
use collab_stream::client::CollabRedisStream;
use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use std::sync::{Arc, Once};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic_proto::history::history_server::HistoryServer;
use tower::layer::util::Identity;
use tracing::info;
use tracing::subscriber::set_global_default;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

pub async fn run_server(
  listener: TcpListener,
  config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
  dotenvy::dotenv().ok();
  // Initialize logger
  init_subscriber(&config.app_env);
  info!("config loaded: {:?}", &config);

  // Start the server
  info!("Starting server at: {:?}", listener.local_addr());
  let stream = TcpListenerStream::new(listener);
  let server = create_app(config).await.unwrap();
  server.serve_with_incoming(stream).await?;
  Ok(())
}

pub fn init_subscriber(app_env: &Environment) {
  static START: Once = Once::new();
  START.call_once(|| {
    let level = std::env::var("RUST_LOG").unwrap_or("info".to_string());
    let mut filters = vec![];
    filters.push(format!("appflowy_history={}", level));
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

pub async fn create_app(config: Config) -> Result<Router<Identity>, Error> {
  // Postgres
  info!("Preparing to run database migrations...");
  let pg_pool = get_connection_pool(&config.db_settings).await?;
  migrate(&pg_pool).await?;

  // Redis
  let redis_connection_manager = redis::Client::open(config.redis_url)
    .expect("failed to create redis client")
    .get_connection_manager()
    .await
    .expect("failed to get redis connection manager");

  let collab_redis_stream =
    CollabRedisStream::new_with_connection_manager(redis_connection_manager.clone());
  let open_collab_manager = Arc::new(
    OpenCollabManager::new(
      collab_redis_stream,
      pg_pool.clone(),
      &config.stream_settings,
    )
    .await,
  );

  let state = AppState {
    redis_client: redis_connection_manager,
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
