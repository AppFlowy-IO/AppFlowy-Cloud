use crate::config::{Config, DatabaseSetting, Environment, S3Setting};
use anyhow::Error;
use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::import_worker::task_queue::run_import_worker;
use aws_sdk_s3::config::{Credentials, Region, SharedCredentialsProvider};

use crate::s3_client::S3Client;
use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use secrecy::ExposeSecret;
use std::sync::Once;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::LocalSet;
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
  create_app(listener, config).await.unwrap();
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

pub async fn create_app(listener: TcpListener, config: Config) -> Result<(), Error> {
  // Postgres
  info!("Preparing to run database migrations...");
  let pg_pool = get_connection_pool(&config.db_settings).await?;

  // Redis
  let redis_client = redis::Client::open(config.redis_url)
    .expect("failed to create redis client")
    .get_connection_manager()
    .await
    .expect("failed to get redis connection manager");

  let s3_client = get_aws_s3_client(&config.s3_setting).await?;

  let state = AppState {
    redis_client,
    pg_pool,
    s3_client,
  };

  let local_set = LocalSet::new();
  let import_worker_fut = local_set.run_until(run_import_worker(
    state.redis_client.clone(),
    state.s3_client.clone(),
    state.pg_pool.clone(),
  ));

  let app = Router::new()
    .route("/health", get(health_check))
    .with_state(state);

  tokio::select! {
    _ = import_worker_fut => {
      info!("Notion importer stopped");
    },
    _ = axum::serve(listener, app) => {
      info!("worker stopped");
    },
  }

  Ok(())
}

async fn health_check() -> StatusCode {
  StatusCode::OK
}

#[derive(Clone)]
pub struct AppState {
  pub redis_client: ConnectionManager,
  pub pg_pool: PgPool,
  pub s3_client: S3Client,
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

pub async fn get_aws_s3_client(s3_setting: &S3Setting) -> Result<S3Client, Error> {
  let credentials = Credentials::new(
    s3_setting.access_key.clone(),
    s3_setting.secret_key.expose_secret().clone(),
    None,
    None,
    "custom",
  );
  let shared_credentials = SharedCredentialsProvider::new(credentials);

  // Configure the AWS SDK
  let config_builder = aws_sdk_s3::Config::builder()
    .credentials_provider(shared_credentials)
    .force_path_style(true)
    .region(Region::new(s3_setting.region.clone()));

  let config = if s3_setting.use_minio {
    config_builder.endpoint_url(&s3_setting.minio_url).build()
  } else {
    config_builder.build()
  };
  let client = aws_sdk_s3::Client::from_conf(config);
  Ok(S3Client {
    inner: client,
    bucket: s3_setting.bucket.clone(),
  })
}
