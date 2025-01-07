use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use access_control::casbin::collab::{CollabAccessControlImpl, RealtimeCollabAccessControlImpl};

use access_control::casbin::workspace::WorkspaceAccessControlImpl;
use actix::Supervisor;
use actix_web::dev::Server;
use actix_web::web::Data;
use actix_web::{App, HttpServer};
use anyhow::{Context, Error};
use aws_sdk_s3::config::{Credentials, Region, SharedCredentialsProvider};
use aws_sdk_s3::operation::create_bucket::CreateBucketError;
use aws_sdk_s3::types::{
  BucketInfo, BucketLocationConstraint, BucketType, CreateBucketConfiguration,
};
use secrecy::{ExposeSecret, Secret};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::info;

use crate::actix_ws::server::RealtimeServerActor;
use crate::api::{collab_scope, ws_scope};
use crate::collab::access_control::CollabStorageAccessControlImpl;
use access_control::casbin::access::AccessControl;
use collab_stream::metrics::CollabStreamMetrics;
use collab_stream::stream_router::{StreamRouter, StreamRouterOptions};
use database::file::s3_client_impl::AwsS3BucketClientImpl;

use crate::collab::cache::CollabCache;
use crate::collab::storage::CollabStorageImpl;
use crate::command::{CLCommandReceiver, CLCommandSender};
use crate::config::{get_env_var, Config, DatabaseSetting, S3Setting};
use crate::pg_listener::PgListeners;
use crate::snapshot::SnapshotControl;
use crate::state::{AppMetrics, AppState, UserCache};
use crate::CollaborationServer;
use indexer::collab_indexer::IndexerProvider;
use indexer::scheduler::{IndexerConfiguration, IndexerScheduler};

pub struct Application {
  actix_server: Server,
}

impl Application {
  pub async fn build(
    config: Config,
    state: AppState,
    rt_cmd_recv: CLCommandReceiver,
  ) -> Result<Self, Error> {
    let address = format!("{}:{}", config.application.host, config.application.port);
    let listener = TcpListener::bind(&address)?;
    info!(
      "Collab Service started at {}",
      listener.local_addr().unwrap()
    );
    let actix_server = run_actix_server(listener, state, config, rt_cmd_recv).await?;

    Ok(Self { actix_server })
  }

  pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
    self.actix_server.await
  }
}

pub async fn run_actix_server(
  listener: TcpListener,
  state: AppState,
  config: Config,
  rt_cmd_recv: CLCommandReceiver,
) -> Result<Server, Error> {
  let storage = state.collab_access_control_storage.clone();

  // Initialize metrics that which are registered in the registry.
  let realtime_server = CollaborationServer::<_>::new(
    storage.clone(),
    Arc::new(RealtimeCollabAccessControlImpl::new(
      state.access_control.clone(),
    )),
    state.metrics.realtime_metrics.clone(),
    rt_cmd_recv,
    state.redis_stream_router.clone(),
    state.redis_connection_manager.clone(),
    Duration::from_secs(config.collab.group_persistence_interval_secs),
    Duration::from_secs(config.collab.group_prune_grace_period_secs),
    state.indexer_scheduler.clone(),
  )
  .await
  .unwrap();
  let realtime_server_actor = Supervisor::start(|_| RealtimeServerActor(realtime_server));
  let mut server = HttpServer::new(move || {
    App::new()
      .app_data(Data::new(state.clone()))
      .app_data(Data::new(state.config.gotrue.jwt_secret.clone()))
      .app_data(Data::new(realtime_server_actor.clone()))
      .service(ws_scope())
      .service(collab_scope())
  });
  server = server.listen(listener)?;

  Ok(server.run())
}

pub async fn init_state(config: &Config, rt_cmd_tx: CLCommandSender) -> Result<AppState, Error> {
  let metrics = AppMetrics::new();
  let pg_pool = get_connection_pool(&config.db_settings).await?;

  // User cache
  let user_cache = UserCache::new(pg_pool.clone()).await;

  info!("Connecting to Redis...");
  let (redis_conn_manager, redis_stream_router) = get_redis_client(
    config.redis_uri.expose_secret(),
    config.redis_worker_count,
    metrics.collab_stream_metrics.clone(),
  )
  .await?;

  // Pg listeners
  info!("Setting up Pg listeners...");
  let pg_listeners = Arc::new(PgListeners::new(&pg_pool).await?);
  let access_control =
    AccessControl::new(pg_pool.clone(), metrics.access_control_metrics.clone()).await?;

  info!("Setting up S3 bucket...");
  let s3_client = AwsS3BucketClientImpl::new(
    get_aws_s3_client(&config.s3).await?,
    config.s3.bucket.clone(),
  );

  let collab_access_control = CollabAccessControlImpl::new(access_control.clone());
  let workspace_access_control = WorkspaceAccessControlImpl::new(access_control.clone());
  let collab_cache = CollabCache::new(
    redis_conn_manager.clone(),
    pg_pool.clone(),
    s3_client.clone(),
    metrics.collab_metrics.clone(),
    config.collab.s3_collab_threshold as usize,
  );

  let collab_storage_access_control = CollabStorageAccessControlImpl {
    collab_access_control: Arc::new(collab_access_control.clone()),
    workspace_access_control: Arc::new(workspace_access_control.clone()),
    cache: collab_cache.clone(),
  };
  let snapshot_control = SnapshotControl::new(
    pg_pool.clone(),
    s3_client.clone(),
    metrics.collab_metrics.clone(),
  )
  .await;
  let collab_storage = Arc::new(CollabStorageImpl::new(
    collab_cache.clone(),
    collab_storage_access_control,
    snapshot_control,
    rt_cmd_tx,
  ));

  info!("Setting up Indexer provider...");
  let embedder_config = IndexerConfiguration {
    enable: get_env_var("APPFLOWY_INDEXER_ENABLED", "true")
      .parse::<bool>()
      .unwrap_or(true),
    openai_api_key: Secret::new(get_env_var("AI_OPENAI_API_KEY", "")),
    embedding_buffer_size: get_env_var("APPFLOWY_INDEXER_EMBEDDING_BUFFER_SIZE", "2000")
      .parse::<usize>()
      .unwrap_or(2000),
  };
  let indexer_scheduler = IndexerScheduler::new(
    IndexerProvider::new(),
    pg_pool.clone(),
    collab_storage.clone(),
    metrics.embedding_metrics.clone(),
    embedder_config,
    redis_conn_manager.clone(),
  );

  let app_state = AppState {
    config: Arc::new(config.clone()),
    pg_listeners,
    user_cache,
    redis_stream_router,
    redis_connection_manager: redis_conn_manager,
    access_control,
    collab_access_control_storage: collab_storage,
    metrics,
    indexer_scheduler,
  };
  Ok(app_state)
}

async fn get_redis_client(
  redis_uri: &str,
  worker_count: usize,
  metrics: Arc<CollabStreamMetrics>,
) -> Result<(redis::aio::ConnectionManager, Arc<StreamRouter>), Error> {
  info!("Connecting to redis with uri: {}", redis_uri);
  let client = redis::Client::open(redis_uri).context("failed to connect to redis")?;

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
  Ok((manager, router.into()))
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
            tracing::error!("Unhandle s3 service error: {:?}", err);
            Err(err.into())
          },
        }
      } else {
        tracing::error!("Failed to create bucket: {:?}", err);
        Ok(())
      }
    },
  }
}
