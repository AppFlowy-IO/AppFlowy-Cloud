use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use actix::Supervisor;
use actix_web::dev::Server;
use actix_web::web::Data;
use actix_web::{App, HttpServer};
use anyhow::{Context, Error};
use secrecy::ExposeSecret;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::info;

use crate::actix_ws::server::RealtimeServerActor;
use access_control::access::AccessControl;
use workspace_access::notification::spawn_listen_on_workspace_member_change;
use workspace_access::WorkspaceAccessControlImpl;

use crate::api::ws_scope;
use crate::collab::access_control::{
  CollabAccessControlImpl, CollabStorageAccessControlImpl, RealtimeCollabAccessControlImpl,
};
use crate::collab::cache::CollabCache;
use crate::collab::notification::spawn_listen_on_collab_member_change;
use crate::collab::storage::CollabStorageImpl;
use crate::command::{CLCommandReceiver, CLCommandSender};
use crate::config::{Config, DatabaseSetting};
use crate::pg_listener::PgListeners;
use crate::snapshot::SnapshotControl;
use crate::state::{AppMetrics, AppState, UserCache};
use crate::CollaborationServer;

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
    info!("Server started at {}", listener.local_addr().unwrap());
    let actix_server = run_actix_server(listener, state, rt_cmd_recv).await?;

    Ok(Self { actix_server })
  }

  pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
    self.actix_server.await
  }
}

pub async fn run_actix_server(
  listener: TcpListener,
  state: AppState,
  rt_cmd_recv: CLCommandReceiver,
) -> Result<Server, anyhow::Error> {
  let storage = state.collab_access_control_storage.clone();

  // Initialize metrics that which are registered in the registry.
  let realtime_server = CollaborationServer::<_, _>::new(
    storage.clone(),
    RealtimeCollabAccessControlImpl::new(state.access_control.clone()),
    state.metrics.realtime_metrics.clone(),
    rt_cmd_recv,
    state.redis_connection_manager.clone(),
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
  let redis_conn_manager = get_redis_client(config.redis_uri.expose_secret()).await?;

  // Pg listeners
  info!("Setting up Pg listeners...");
  let pg_listeners = Arc::new(PgListeners::new(&pg_pool).await?);
  let access_control =
    AccessControl::new(pg_pool.clone(), metrics.access_control_metrics.clone()).await?;
  let collab_member_listener = pg_listeners.subscribe_collab_member_change();
  let workspace_member_listener = pg_listeners.subscribe_workspace_member_change();

  spawn_listen_on_workspace_member_change(workspace_member_listener, access_control.clone());
  spawn_listen_on_collab_member_change(
    pg_pool.clone(),
    collab_member_listener,
    access_control.clone(),
  );

  let collab_access_control = CollabAccessControlImpl::new(access_control.clone());
  let workspace_access_control = WorkspaceAccessControlImpl::new(access_control.clone());
  let collab_cache = CollabCache::new(redis_conn_manager.clone(), pg_pool.clone());

  let collab_storage_access_control = CollabStorageAccessControlImpl {
    collab_access_control: collab_access_control.clone().into(),
    workspace_access_control: workspace_access_control.clone().into(),
    cache: collab_cache.clone(),
  };
  let snapshot_control = SnapshotControl::new(
    redis_conn_manager.clone(),
    pg_pool.clone(),
    metrics.collab_metrics.clone(),
  )
  .await;
  let collab_storage = Arc::new(CollabStorageImpl::new(
    collab_cache.clone(),
    collab_storage_access_control,
    snapshot_control,
    rt_cmd_tx,
    redis_conn_manager.clone(),
    metrics.collab_metrics.clone(),
  ));
  let app_state = AppState {
    config: Arc::new(config.clone()),
    pg_listeners,
    user_cache,
    redis_connection_manager: redis_conn_manager,
    access_control,
    collab_access_control_storage: collab_storage,
    metrics,
  };
  Ok(app_state)
}

async fn get_redis_client(redis_uri: &str) -> Result<redis::aio::ConnectionManager, Error> {
  info!("Connecting to redis with uri: {}", redis_uri);
  let manager = redis::Client::open(redis_uri)
    .context("failed to connect to redis")?
    .get_connection_manager()
    .await
    .context("failed to get the connection manager")?;
  Ok(manager)
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
