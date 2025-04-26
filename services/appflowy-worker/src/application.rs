use crate::config::{Config, DatabaseSetting, Environment, S3Setting};
use anyhow::Error;
use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::import_worker::worker::run_import_worker;
use aws_sdk_s3::config::{Credentials, Region, SharedCredentialsProvider};

use crate::import_worker::email_notifier::EmailNotifier;
use crate::s3_client::S3ClientImpl;

use axum::Router;

use crate::mailer::AFWorkerMailer;
use crate::metric::ImportMetrics;
use appflowy_worker::indexer_worker::{run_background_indexer, BackgroundIndexerConfig};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use indexer::metrics::EmbeddingMetrics;
use indexer::vector::embedder::get_open_ai_config;
use infra::env_util::get_env_var;
use mailer::sender::Mailer;
use secrecy::ExposeSecret;
use std::sync::{Arc, Once};
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
    let env_filter = EnvFilter::from_default_env();

    let builder = tracing_subscriber::fmt()
      .with_target(true)
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
  let redis_client = redis::Client::open(config.redis_url.clone())
    .expect("failed to create redis client")
    .get_connection_manager()
    .await
    .expect("failed to get redis connection manager");

  let mailer = get_worker_mailer(&config).await?;
  let s3_client = get_aws_s3_client(&config.s3_setting).await?;
  let metrics = AppMetrics::new();

  let state = AppState {
    redis_client,
    pg_pool,
    s3_client,
    mailer: mailer.clone(),
    metrics,
  };

  let local_set = LocalSet::new();
  let email_notifier = EmailNotifier::new(mailer);
  let tick_interval = get_env_var("APPFLOWY_WORKER_IMPORT_TICK_INTERVAL", "10")
    .parse::<u64>()
    .unwrap_or(10);

  // Maximum file size for import
  let maximum_import_file_size =
    get_env_var("APPFLOWY_WORKER_MAX_IMPORT_FILE_SIZE", "1_000_000_000")
      .parse::<u64>()
      .unwrap_or(1_000_000_000);

  let import_worker_fut = local_set.run_until(run_import_worker(
    state.pg_pool.clone(),
    state.redis_client.clone(),
    Some(state.metrics.import_metrics.clone()),
    Arc::new(state.s3_client.clone()),
    Arc::new(email_notifier),
    "import_task_stream",
    tick_interval,
    maximum_import_file_size,
  ));

  let (open_ai_config, azure_ai_config) = get_open_ai_config();
  let indexer_config = BackgroundIndexerConfig {
    enable: appflowy_collaborate::config::get_env_var("APPFLOWY_INDEXER_ENABLED", "true")
      .parse::<bool>()
      .unwrap_or(true),
    open_ai_config,
    azure_ai_config,
    tick_interval_secs: 10,
  };

  tokio::spawn(run_background_indexer(
    state.pg_pool.clone(),
    state.redis_client.clone(),
    state.metrics.embedder_metrics.clone(),
    indexer_config,
  ));

  let app = Router::new()
    .route("/metrics", get(metrics_handler))
    .with_state(Arc::new(state));

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

#[derive(Clone)]
pub struct AppState {
  pub redis_client: ConnectionManager,
  pub pg_pool: PgPool,
  pub s3_client: S3ClientImpl,
  #[allow(dead_code)]
  pub mailer: AFWorkerMailer,
  pub metrics: AppMetrics,
}

async fn get_worker_mailer(config: &Config) -> Result<AFWorkerMailer, Error> {
  let mailer = Mailer::new(
    config.mailer.smtp_username.clone(),
    config.mailer.smtp_email.clone(),
    config.mailer.smtp_password.clone(),
    &config.mailer.smtp_host,
    config.mailer.smtp_port,
    config.mailer.smtp_tls_kind.as_str(),
  )
  .await?;

  AFWorkerMailer::new(mailer).await
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

pub async fn get_aws_s3_client(s3_setting: &S3Setting) -> Result<S3ClientImpl, Error> {
  let credentials = Credentials::new(
    s3_setting.access_key.clone(),
    s3_setting.secret_key.expose_secret().clone(),
    None,
    None,
    "appflowy-worker",
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
  Ok(S3ClientImpl {
    inner: client,
    bucket: s3_setting.bucket.clone(),
  })
}

#[derive(Clone)]
pub struct AppMetrics {
  #[allow(dead_code)]
  registry: Arc<prometheus_client::registry::Registry>,
  import_metrics: Arc<ImportMetrics>,
  embedder_metrics: Arc<EmbeddingMetrics>,
}

impl AppMetrics {
  pub fn new() -> Self {
    let mut registry = prometheus_client::registry::Registry::default();
    let import_metrics = Arc::new(ImportMetrics::register(&mut registry));
    let embedder_metrics = Arc::new(EmbeddingMetrics::register(&mut registry));
    Self {
      registry: Arc::new(registry),
      import_metrics,
      embedder_metrics,
    }
  }
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
  let mut buffer = String::new();
  if let Err(err) = prometheus_client::encoding::text::encode(&mut buffer, &state.metrics.registry)
  {
    return (
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Failed to encode metrics: {:?}", err),
    )
      .into_response();
  }
  (StatusCode::OK, buffer).into_response()
}
