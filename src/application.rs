use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use access_control::casbin::access::AccessControl;
use access_control::casbin::collab::{CollabAccessControlImpl, RealtimeCollabAccessControlImpl};
use access_control::casbin::workspace::WorkspaceAccessControlImpl;
use access_control::collab::{CollabAccessControl, RealtimeAccessControl};
use access_control::noops::collab::{
  CollabAccessControlImpl as NoOpsCollabAccessControlImpl,
  RealtimeCollabAccessControlImpl as NoOpsRealtimeCollabAccessControlImpl,
};
use access_control::noops::workspace::WorkspaceAccessControlImpl as NoOpsWorkspaceAccessControlImpl;
use access_control::workspace::WorkspaceAccessControl;
use actix::{Actor, Supervisor};
#[cfg(feature = "use_actix_cors")]
use actix_cors::Cors;
use actix_identity::IdentityMiddleware;
use actix_session::storage::RedisSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::Key;
use actix_web::middleware::NormalizePath;
use actix_web::{dev::Server, web, web::Data, App, HttpResponse, HttpServer, Responder};
use anyhow::{Context, Error};
use aws_sdk_s3::config::{Credentials, Region, SharedCredentialsProvider};
use aws_sdk_s3::operation::create_bucket::CreateBucketError;
use aws_sdk_s3::types::{
  BucketInfo, BucketLocationConstraint, BucketType, CreateBucketConfiguration,
};
use mailer::config::MailerSetting;
use secrecy::ExposeSecret;
use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio::sync::RwLock;
use tracing::{error, info};

use appflowy_ai_client::client::AppFlowyAIClient;
use appflowy_collaborate::actix_ws::server::RealtimeServerActor;
use appflowy_collaborate::collab::cache::CollabCache;
use appflowy_collaborate::collab::collab_store::CollabStoreImpl;
use appflowy_collaborate::ws2::{CollabManager, WsServer};
use appflowy_collaborate::CollaborationServer;
use collab_stream::awareness_gossip::AwarenessGossip;
use collab_stream::metrics::CollabStreamMetrics;
use collab_stream::stream_router::{StreamRouter, StreamRouterOptions};
use database::file::s3_client_impl::{AwsS3BucketClientImpl, S3BucketStorage};
use indexer::collab_indexer::IndexerProvider;
use indexer::scheduler::{IndexerConfiguration, IndexerScheduler};
use indexer::vector::embedder::get_open_ai_config;
use infra::env_util::get_env_var;
use infra::thread_pool::ThreadPoolNoAbortBuilder;
use mailer::sender::Mailer;
use snowflake::Snowflake;

use crate::api::access_request::access_request_scope;
use crate::api::ai::ai_completion_scope;
use crate::api::chat::chat_scope;
use crate::api::data_import::data_import_scope;
use crate::api::file_storage::file_storage_scope;
use crate::api::guest::sharing_scope;
use crate::api::invite_code::invite_code_scope;
use crate::api::metrics::metrics_scope;
use crate::api::search::search_scope;
use crate::api::server_info::server_info_scope;
use crate::api::template::template_scope;
use crate::api::user::user_scope;
use crate::api::workspace::{collab_scope, workspace_scope};
use crate::api::ws::ws_scope;
use crate::biz::pg_listener::PgListeners;
use crate::biz::workspace::publish::{
  PublishedCollabPostgresStore, PublishedCollabS3StoreWithPostgresFallback, PublishedCollabStore,
};
use crate::config::config::{
  Config, DatabaseSetting, GoTrueSetting, PublishedCollabStorageBackend, S3Setting,
};
use crate::mailer::AFCloudMailer;
use crate::middleware::metrics_mw::MetricsMiddleware;
use crate::middleware::request_id::RequestIdMiddleware;
use crate::state::{AppMetrics, AppState, GoTrueAdmin, UserCache};

pub struct Application {
  port: u16,
  actix_server: Server,
}

impl Application {
  pub async fn build(config: Config, state: AppState) -> Result<Self, Error> {
    let address = format!("{}:{}", config.application.host, config.application.port);
    let listener = TcpListener::bind(&address)?;
    let port = listener.local_addr().unwrap().port();
    info!("Server started at {}", listener.local_addr().unwrap());
    let actix_server = run_actix_server(listener, state, config).await?;

    Ok(Self { port, actix_server })
  }

  pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
    self.actix_server.await
  }

  pub fn port(&self) -> u16 {
    self.port
  }
}

pub async fn run_actix_server(
  listener: TcpListener,
  state: AppState,
  config: Config,
) -> Result<Server, Error> {
  let redis_store = RedisSessionStore::new(config.redis_uri.expose_secret())
    .await
    .map_err(|e| {
      anyhow::anyhow!(
        "Failed to connect to Redis at {:?}: {:?}",
        config.redis_uri,
        e
      )
    })?;

  let storage = state.collab_storage.clone();

  // Initialize metrics that which are registered in the registry.
  let realtime_server = CollaborationServer::new(
    storage.clone(),
    state.realtime_access_control.clone(),
    state.metrics.realtime_metrics.clone(),
    state.redis_stream_router.clone(),
    state.awareness_gossip.clone(),
    state.redis_connection_manager.clone(),
    Duration::from_secs(config.collab.group_persistence_interval_secs),
    state.indexer_scheduler.clone(),
  )
  .await
  .unwrap();

  let realtime_server_actor = Supervisor::start(|_| RealtimeServerActor(realtime_server));
  let mut server = HttpServer::new(move || {
    let app = App::new()
      .wrap(NormalizePath::trim())
       // Middleware is registered for each App, scope, or Resource and executed in opposite order as registration
      .wrap(MetricsMiddleware)
      .wrap(IdentityMiddleware::default())
      .wrap(
        SessionMiddleware::builder(redis_store.clone(), Key::generate())
          .build(),
      )
      .wrap(RequestIdMiddleware);

    #[cfg(feature = "use_actix_cors")]
    let app = app.wrap(actix_cors_scope());

    app
      .service(server_info_scope())
      .service(user_scope())
      .service(workspace_scope())
      .service(invite_code_scope())
      .service(collab_scope())
      .service(ws_scope())
      .service(file_storage_scope())
      .service(chat_scope())
      .service(ai_completion_scope())
      .service(metrics_scope())
      .service(search_scope())
      .service(template_scope())
      .service(data_import_scope())
      .service(access_request_scope())
      .service(sharing_scope())
      .route("/health", web::get().to(health_check))
      .app_data(Data::new(state.metrics.registry.clone()))
      .app_data(Data::new(state.metrics.request_metrics.clone()))
      .app_data(Data::new(state.metrics.realtime_metrics.clone()))
      .app_data(Data::new(state.metrics.access_control_metrics.clone()))
      .app_data(Data::new(realtime_server_actor.clone()))
      .app_data(Data::new(state.config.gotrue.jwt_secret.clone()))
      .app_data(Data::new(state.clone()))
      .app_data(Data::new(storage.clone()))
      .app_data(Data::new(state.published_collab_store.clone()))
  });

  server = server.listen(listener)?;

  Ok(server.run())
}

pub async fn init_state(config: &Config) -> Result<AppState, Error> {
  // Print the feature flags

  let metrics = AppMetrics::new();

  // Postgres
  info!("Preparing to run database migrations...");
  let pg_pool = get_connection_pool(&config.db_settings).await?;
  migrate(&pg_pool).await?;

  // Bucket storage
  info!("Setting up S3 bucket...");
  let s3_client = AwsS3BucketClientImpl::new(
    get_aws_s3_client(&config.s3).await?,
    config.s3.bucket.clone(),
    config.s3.minio_url.clone(),
    config.s3.presigned_url_endpoint.clone(),
  );
  let bucket_storage = Arc::new(S3BucketStorage::from_bucket_impl(
    s3_client.clone(),
    pg_pool.clone(),
  ));

  // Published Collab Storage
  info!("Setting up Published Collab storage...");
  let published_collab_store: Arc<dyn PublishedCollabStore> =
    match config.published_collab.storage_backend {
      PublishedCollabStorageBackend::Postgres => {
        info!("Using Postgres as the Published Collab storage backend ...");
        Arc::new(PublishedCollabPostgresStore::new(
          metrics.published_collab_metrics.clone(),
          pg_pool.clone(),
        ))
      },
      PublishedCollabStorageBackend::S3WithPostgresBackup => {
        info!("Using S3 as the Published Collab storage backend with Postgres as the backup ...");
        Arc::new(PublishedCollabS3StoreWithPostgresFallback::new(
          metrics.published_collab_metrics.clone(),
          pg_pool.clone(),
          s3_client.clone(),
        ))
      },
    };

  // Gotrue
  info!("Connecting to GoTrue...");
  let gotrue_client = get_gotrue_client(&config.gotrue).await?;
  let gotrue_admin = get_admin_client(gotrue_client.clone(), &config.gotrue);

  // Redis
  info!("Connecting to Redis...");
  let (redis_conn_manager, redis_stream_router, awareness_gossip) = get_redis_client(
    config.redis_uri.expose_secret(),
    config.redis_worker_count,
    metrics.collab_stream_metrics.clone(),
  )
  .await?;

  info!("Setup AppFlowy AI: {}", config.appflowy_ai.url());
  let appflowy_ai_client = AppFlowyAIClient::new(&config.appflowy_ai.url());
  // Pg listeners
  info!("Setting up Pg listeners...");
  let pg_listeners = Arc::new(PgListeners::new(&pg_pool).await?);
  // let collab_member_listener = pg_listeners.subscribe_collab_member_change();

  let use_redis_ac_cache = get_env_var("APPFLOWY_ACCESS_CONTROL_REDIS_CACHE_ENABLED", "false")
    .parse::<bool>()
    .unwrap_or(false);

  info!(
    "Setting up access controls, is_enable: {}, use redis cache: {}",
    &config.access_control.is_enabled, use_redis_ac_cache,
  );

  let redis_uri = if use_redis_ac_cache {
    Some(config.redis_uri.expose_secret().as_str())
  } else {
    None
  };

  let access_control = AccessControl::new(
    pg_pool.clone(),
    redis_uri,
    metrics.access_control_metrics.clone(),
  )
  .await?;

  let user_cache = UserCache::new(pg_pool.clone()).await;
  let collab_access_control: Arc<dyn CollabAccessControl> =
    if config.access_control.is_enabled && config.access_control.enable_collab_access_control {
      Arc::new(CollabAccessControlImpl::new(access_control.clone()))
    } else {
      Arc::new(NoOpsCollabAccessControlImpl::new())
    };
  let workspace_access_control: Arc<dyn WorkspaceAccessControl> =
    if config.access_control.is_enabled && config.access_control.enable_workspace_access_control {
      Arc::new(WorkspaceAccessControlImpl::new(access_control.clone()))
    } else {
      Arc::new(NoOpsWorkspaceAccessControlImpl::new())
    };

  // thread pool
  let thread_pool = Arc::new(
    ThreadPoolNoAbortBuilder::new()
      .thread_name(|idx| format!("af-collab-worker-{}", idx))
      .num_threads(4)
      .build()
      .expect("Failed to create collab thread pool"),
  );

  let realtime_access_control: Arc<dyn RealtimeAccessControl> =
    if config.access_control.is_enabled && config.access_control.enable_realtime_access_control {
      Arc::new(RealtimeCollabAccessControlImpl::new(access_control))
    } else {
      Arc::new(NoOpsRealtimeCollabAccessControlImpl::new())
    };
  let collab_cache = CollabCache::new(
    thread_pool.clone(),
    redis_conn_manager.clone(),
    pg_pool.clone(),
    s3_client.clone(),
    metrics.collab_metrics.clone(),
    config.collab.s3_collab_threshold as usize,
  );

  let collab_access_control_storage = Arc::new(CollabStoreImpl::new(
    collab_cache.clone(),
    collab_access_control.clone(),
    workspace_access_control.clone(),
  ));

  let mailer = get_mailer(&config.mailer).await?;

  info!("Setting up Indexer scheduler...");
  let (open_ai_config, azure_ai_config) = get_open_ai_config();
  let embedder_config = IndexerConfiguration {
    enable: get_env_var("APPFLOWY_INDEXER_ENABLED", "true")
      .parse::<bool>()
      .unwrap_or(true),
    open_ai_config,
    azure_ai_config,
    embedding_buffer_size: appflowy_collaborate::config::get_env_var(
      "APPFLOWY_INDEXER_EMBEDDING_BUFFER_SIZE",
      "5000",
    )
    .parse::<usize>()
    .unwrap_or(5000),
  };
  let indexer_scheduler = IndexerScheduler::new(
    IndexerProvider::new(),
    pg_pool.clone(),
    collab_access_control_storage.clone(),
    metrics.embedding_metrics.clone(),
    embedder_config,
    redis_conn_manager.clone(),
  );
  let manager = CollabManager::new(
    thread_pool.clone(),
    collab_access_control.clone(),
    collab_cache.clone(),
    redis_conn_manager.clone(),
    redis_stream_router.clone(),
    awareness_gossip.clone(),
    indexer_scheduler.clone(),
  );
  let ws_server = WsServer::new(manager).start();

  info!("Application state initialized");
  Ok(AppState {
    pg_pool,
    config: Arc::new(config.clone()),
    user_cache,
    id_gen: Arc::new(RwLock::new(Snowflake::new(1))),
    gotrue_client,
    redis_stream_router,
    awareness_gossip,
    redis_connection_manager: redis_conn_manager,
    collab_cache,
    collab_storage: collab_access_control_storage,
    collab_access_control,
    workspace_access_control,
    realtime_access_control,
    bucket_storage,
    published_collab_store,
    bucket_client: s3_client,
    pg_listeners,
    metrics,
    gotrue_admin,
    mailer,
    ai_client: appflowy_ai_client,
    indexer_scheduler,
    ws_server,
  })
}

fn get_admin_client(
  gotrue_client: gotrue::api::Client,
  gotrue_setting: &GoTrueSetting,
) -> GoTrueAdmin {
  GoTrueAdmin::new(
    gotrue_setting.jwt_secret.expose_secret().to_owned(),
    gotrue_setting.service_role.clone(),
    gotrue_client.clone(),
  )
}

async fn get_redis_client(
  redis_uri: &str,
  worker_count: usize,
  metrics: Arc<CollabStreamMetrics>,
) -> Result<
  (
    redis::aio::ConnectionManager,
    Arc<StreamRouter>,
    Arc<AwarenessGossip>,
  ),
  Error,
> {
  info!("Connecting to redis with uri: {}", redis_uri);
  let client = redis::Client::open(redis_uri).context("failed to connect to redis")?;

  let awareness_gossip = AwarenessGossip::new(&client).await?;
  let router = StreamRouter::with_options(
    &client,
    metrics,
    StreamRouterOptions {
      worker_count,
      xread_streams: 100,
      xread_block_millis: Some(5000),
      xread_count: None,
    },
  )?;

  let manager = client
    .get_connection_manager()
    .await
    .context("failed to get the connection manager")?;
  Ok((manager, router.into(), awareness_gossip.into()))
}

pub async fn get_aws_s3_client(s3_setting: &S3Setting) -> Result<aws_sdk_s3::Client, Error> {
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
  if s3_setting.create_bucket {
    create_bucket_if_not_exists(&client, s3_setting).await?;
  } else {
    info!("Skipping bucket creation, assumed to be created externally");
  }
  Ok(client)
}

async fn create_bucket_if_not_exists(
  client: &aws_sdk_s3::Client,
  s3_setting: &S3Setting,
) -> Result<(), Error> {
  let bucket_cfg = if s3_setting.use_minio {
    CreateBucketConfiguration::builder()
      .bucket(BucketInfo::builder().r#type(BucketType::Directory).build())
      .build()
  } else {
    CreateBucketConfiguration::builder()
      .location_constraint(BucketLocationConstraint::from(s3_setting.region.as_str()))
      .build()
  };

  match client
    .create_bucket()
    .bucket(&s3_setting.bucket)
    .create_bucket_configuration(bucket_cfg)
    .send()
    .await
  {
    Ok(_) => {
      info!(
        "bucket created successfully: {}, region: {}",
        s3_setting.bucket, s3_setting.region
      );
      Ok(())
    },
    Err(err) => {
      if let Some(service_error) = err.as_service_error() {
        match service_error {
          CreateBucketError::BucketAlreadyOwnedByYou(_)
          | CreateBucketError::BucketAlreadyExists(_) => {
            info!("Bucket already exists");
            Ok(())
          },
          _ => {
            error!("Unhandle s3 service error: {:?}", err);
            Err(err.into())
          },
        }
      } else {
        error!("Failed to create bucket: {:?}", err);
        Err(err.into())
      }
    },
  }
}

async fn get_mailer(mailer: &MailerSetting) -> Result<AFCloudMailer, Error> {
  info!("Connecting to mailer with setting: {:?}", mailer);
  let mailer = Mailer::new(
    mailer.smtp_username.clone(),
    mailer.smtp_email.clone(),
    mailer.smtp_password.clone(),
    &mailer.smtp_host,
    mailer.smtp_port,
    mailer.smtp_tls_kind.as_str(),
  )
  .await?;

  AFCloudMailer::new(mailer).await
}

async fn get_connection_pool(setting: &DatabaseSetting) -> Result<PgPool, Error> {
  info!("Connecting to postgres database with setting: {}", setting);
  PgPoolOptions::new()
    .max_connections(setting.max_connections)
    .acquire_timeout(Duration::from_secs(10))
    .max_lifetime(Duration::from_secs(30 * 60))
    .idle_timeout(Duration::from_secs(30))
    .connect_with(setting.pg_connect_options())
    .await
    .map_err(|e| anyhow::anyhow!("Failed to connect to postgres database: {}", e))
}

async fn migrate(pool: &PgPool) -> Result<(), Error> {
  sqlx::migrate!("./migrations")
    .set_ignore_missing(true)
    .run(pool)
    .await
    .map_err(|e| anyhow::anyhow!("Failed to run migrations: {}", e))
}

async fn get_gotrue_client(setting: &GoTrueSetting) -> Result<gotrue::api::Client, Error> {
  info!("Connecting to GoTrue with setting: {:?}", setting);
  let gotrue_client = gotrue::api::Client::new(reqwest::Client::new(), &setting.base_url);
  let _ = gotrue_client
    .health()
    .await
    .map_err(|e| anyhow::anyhow!("Failed to connect to GoTrue: {}", e));
  Ok(gotrue_client)
}

async fn health_check() -> impl Responder {
  HttpResponse::Ok().body("OK")
}

#[cfg(feature = "use_actix_cors")]
fn actix_cors_scope() -> actix_cors::Cors {
  Cors::default()
    .allowed_origin(&get_env_var(
      "APPFLOWY_CORS_ALLOWED_ORIGIN",
      "http://localhost:3000",
    ))
    .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"])
    .allowed_headers(vec![
      "Content-Type",
      "Authorization",
      "Accept",
      "Client-Version",
      "Device-Id",
      "X-Request-Id",
    ])
    .max_age(3600)
}
