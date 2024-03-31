use crate::api::metrics::metrics_scope;

use crate::api::file_storage::file_storage_scope;
use crate::api::user::user_scope;
use crate::api::workspace::{collab_scope, workspace_scope};
use crate::api::ws::ws_scope;
use access_control::access::{enable_access_control, AccessControl};

use crate::biz::actix_ws::server::RealtimeServerActor;
use crate::biz::casbin::{
  CollabAccessControlImpl, RealtimeCollabAccessControlImpl, WorkspaceAccessControlImpl,
};
use crate::biz::collab::access_control::{
  CollabMiddlewareAccessControl, CollabStorageAccessControlImpl,
};
use crate::biz::collab::cache::CollabCache;
use crate::biz::collab::storage::CollabStorageImpl;
use crate::biz::pg_listener::PgListeners;
use crate::biz::snapshot::SnapshotControl;
use crate::biz::workspace::access_control::WorkspaceMiddlewareAccessControl;
use crate::config::config::{Config, DatabaseSetting, GoTrueSetting, S3Setting};
use crate::middleware::access_control_mw::MiddlewareAccessControlTransform;
use crate::middleware::metrics_mw::MetricsMiddleware;
use crate::middleware::request_id::RequestIdMiddleware;
use crate::self_signed::create_self_signed_certificate;
use crate::state::{AppMetrics, AppState, GoTrueAdmin, UserCache};
use actix::Actor;
use actix_identity::IdentityMiddleware;
use actix_session::storage::RedisSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::Key;
use actix_web::{dev::Server, web, web::Data, App, HttpServer};
use anyhow::{Context, Error};
use collab_rt::command::{RTCommandReceiver, RTCommandSender};
use collab_rt::CollabRealtimeServer;
use database::file::bucket_s3_impl::S3BucketStorage;
use openssl::ssl::{SslAcceptor, SslAcceptorBuilder, SslFiletype, SslMethod};
use openssl::x509::X509;
use secrecy::{ExposeSecret, Secret};
use snowflake::Snowflake;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::info;

pub struct Application {
  port: u16,
  server: Server,
}

impl Application {
  pub async fn build(
    config: Config,
    state: AppState,
    rt_cmd_recv: RTCommandReceiver,
  ) -> Result<Self, Error> {
    let address = format!("{}:{}", config.application.host, config.application.port);
    let listener = TcpListener::bind(&address)?;
    let port = listener.local_addr().unwrap().port();
    let server = run(listener, state, config, rt_cmd_recv).await?;

    Ok(Self { port, server })
  }

  pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
    self.server.await
  }

  pub fn port(&self) -> u16 {
    self.port
  }
}

pub async fn run(
  listener: TcpListener,
  state: AppState,
  config: Config,
  rt_cmd_recv: RTCommandReceiver,
) -> Result<Server, anyhow::Error> {
  let redis_store = RedisSessionStore::new(config.redis_uri.expose_secret())
    .await
    .map_err(|e| {
      anyhow::anyhow!(
        "Failed to connect to Redis at {:?}: {:?}",
        config.redis_uri,
        e
      )
    })?;
  let pair = get_certificate_and_server_key(&config);
  let key = pair
    .as_ref()
    .map(|(_, server_key)| Key::from(server_key.expose_secret().as_bytes()))
    .unwrap_or_else(Key::generate);

  let storage = state.collab_access_control_storage.clone();
  let access_control = MiddlewareAccessControlTransform::new()
    .with_acs(WorkspaceMiddlewareAccessControl::new(
      state.pg_pool.clone(),
      state.workspace_access_control.clone().into(),
    ))
    .with_acs(CollabMiddlewareAccessControl::new(
      state.collab_access_control.clone().into(),
      state.collab_cache.clone(),
    ));

  // Initialize metrics that which are registered in the registry.
  let realtime_server = CollabRealtimeServer::<_, _>::new(
    storage.clone(),
    RealtimeCollabAccessControlImpl::new(state.access_control.clone()),
    state.metrics.realtime_metrics.clone(),
    rt_cmd_recv,
  )
  .unwrap();

  let realtime_server_actor = RealtimeServerActor(realtime_server).start();

  let mut server = HttpServer::new(move || {
    App::new()
       // Middleware is registered for each App, scope, or Resource and executed in opposite order as registration
      .wrap(MetricsMiddleware)
      .wrap(IdentityMiddleware::default())
      .wrap(
        SessionMiddleware::builder(redis_store.clone(), key.clone())
          .build(),
      )
      // .wrap(DecryptPayloadMiddleware)
      .wrap(access_control.clone())
      .wrap(RequestIdMiddleware)
      .app_data(web::JsonConfig::default().limit(5 * 1024 * 1024))
      .service(user_scope())
      .service(workspace_scope())
      .service(collab_scope())
      .service(ws_scope())
      .service(file_storage_scope())
      .service(metrics_scope())
      .app_data(Data::new(state.metrics.registry.clone()))
      .app_data(Data::new(state.metrics.request_metrics.clone()))
      .app_data(Data::new(state.metrics.realtime_metrics.clone()))
      .app_data(Data::new(state.metrics.access_control_metrics.clone()))
      .app_data(Data::new(realtime_server_actor.clone()))
      .app_data(Data::new(state.clone()))
      .app_data(Data::new(storage.clone()))
  });

  server = match pair {
    None => server.listen(listener)?,
    Some((certificate, _)) => {
      server.listen_openssl(listener, make_ssl_acceptor_builder(certificate))?
    },
  };

  Ok(server.run())
}

fn get_certificate_and_server_key(config: &Config) -> Option<(Secret<String>, Secret<String>)> {
  if config.application.use_tls {
    Some(create_self_signed_certificate().unwrap())
  } else {
    None
  }
}

pub async fn init_state(config: &Config, rt_cmd_tx: RTCommandSender) -> Result<AppState, Error> {
  // Print the feature flags

  let metrics = AppMetrics::new();

  // Postgres
  info!("Preparing to run database migrations...");
  let pg_pool = get_connection_pool(&config.db_settings).await?;
  migrate(&pg_pool).await?;

  // Bucket storage
  info!("Setting up S3 bucket...");
  let s3_bucket = get_aws_s3_bucket(&config.s3).await?;
  let bucket_storage = Arc::new(S3BucketStorage::from_s3_bucket(s3_bucket, pg_pool.clone()));

  // Gotrue
  info!("Connecting to GoTrue...");
  let gotrue_client = get_gotrue_client(&config.gotrue).await?;
  let gotrue_admin = setup_admin_account(&gotrue_client, &pg_pool, &config.gotrue).await?;

  // Redis
  info!("Connecting to Redis...");
  let redis_client = get_redis_client(config.redis_uri.expose_secret()).await?;

  #[cfg(feature = "ai_enable")]
  let appflowy_ai_client =
    appflowy_ai::client::AppFlowyAIClient::new(config.appflowy_ai.url.expose_secret());

  // Pg listeners
  info!("Setting up Pg listeners...");
  let pg_listeners = Arc::new(PgListeners::new(&pg_pool).await?);
  // let collab_member_listener = pg_listeners.subscribe_collab_member_change();
  // let workspace_member_listener = pg_listeners.subscribe_workspace_member_change();

  info!(
    "Setting up access controls, is_enable: {}",
    enable_access_control()
  );
  let access_control =
    AccessControl::new(pg_pool.clone(), metrics.access_control_metrics.clone()).await?;

  // spawn_listen_on_workspace_member_change(workspace_member_listener, access_control.clone());
  // spawn_listen_on_collab_member_change(
  //   pg_pool.clone(),
  //   collab_member_listener,
  //   access_control.clone(),
  // );

  let user_cache = UserCache::new(pg_pool.clone()).await;
  let collab_access_control = CollabAccessControlImpl::new(access_control.clone());
  let workspace_access_control = WorkspaceAccessControlImpl::new(access_control.clone());
  let collab_cache = CollabCache::new(redis_client.clone(), pg_pool.clone());

  let collab_storage_access_control = CollabStorageAccessControlImpl {
    collab_access_control: collab_access_control.clone().into(),
    workspace_access_control: workspace_access_control.clone().into(),
    cache: collab_cache.clone(),
  };
  let snapshot_control = SnapshotControl::new(
    redis_client.clone(),
    pg_pool.clone(),
    metrics.collab_metrics.clone(),
  )
  .await;
  let collab_storage = Arc::new(CollabStorageImpl::new(
    collab_cache.clone(),
    collab_storage_access_control,
    snapshot_control,
    rt_cmd_tx,
  ));

  info!("Application state initialized");
  Ok(AppState {
    pg_pool,
    config: Arc::new(config.clone()),
    user_cache,
    id_gen: Arc::new(RwLock::new(Snowflake::new(1))),
    gotrue_client,
    redis_client,
    collab_cache,
    collab_access_control_storage: collab_storage,
    collab_access_control,
    workspace_access_control,
    bucket_storage,
    pg_listeners,
    access_control,
    metrics,
    gotrue_admin,
    #[cfg(feature = "ai_enable")]
    appflowy_ai_client,
  })
}

async fn setup_admin_account(
  gotrue_client: &gotrue::api::Client,
  pg_pool: &PgPool,
  gotrue_setting: &GoTrueSetting,
) -> Result<GoTrueAdmin, Error> {
  let admin_email = gotrue_setting.admin_email.as_str();
  let password = gotrue_setting.admin_password.expose_secret();
  let gotrue_admin = GoTrueAdmin::new(admin_email.to_owned(), password.to_owned());

  let res_resp = gotrue_client.sign_up(admin_email, password, None).await;
  match res_resp {
    Err(err) => {
      if let app_error::gotrue::GoTrueError::Internal(err) = err {
        match (err.code, err.msg.as_str()) {
          (400, "User already registered") => {
            info!("Admin user already registered");
            Ok(gotrue_admin)
          },
          _ => Err(err.into()),
        }
      } else {
        Err(err.into())
      }
    },
    Ok(resp) => {
      let admin_user = {
        match resp {
          gotrue_entity::dto::SignUpResponse::Authenticated(resp) => resp.user,
          gotrue_entity::dto::SignUpResponse::NotAuthenticated(user) => user,
        }
      };
      match admin_user.role.as_str() {
        "supabase_admin" => {
          info!("Admin user already created and set role to supabase_admin");
          Ok(gotrue_admin)
        },
        _ => {
          let user_id = admin_user.id.parse::<uuid::Uuid>()?;
          let result = sqlx::query!(
            r#"
            UPDATE auth.users
            SET role = 'supabase_admin', email_confirmed_at = NOW()
            WHERE id = $1
            "#,
            user_id,
          )
          .execute(pg_pool)
          .await
          .context("failed to update the admin user")?;

          assert_eq!(result.rows_affected(), 1);
          info!("Admin user created and set role to supabase_admin");

          Ok(gotrue_admin)
        },
      }
    },
  }
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

async fn get_aws_s3_bucket(s3_setting: &S3Setting) -> Result<s3::Bucket, Error> {
  info!("Connecting to S3 bucket with setting: {:?}", &s3_setting);
  let region = {
    match s3_setting.use_minio {
      true => s3::Region::Custom {
        region: s3_setting.region.to_owned(),
        endpoint: s3_setting.minio_url.to_owned(),
      },
      false => s3_setting
        .region
        .parse::<s3::Region>()
        .context("failed to parser s3 setting")?,
    }
  };

  let cred = s3::creds::Credentials {
    access_key: Some(s3_setting.access_key.to_owned()),
    secret_key: Some(s3_setting.secret_key.expose_secret().to_owned()),
    security_token: None,
    session_token: None,
    expiration: None,
  };

  match s3::Bucket::create_with_path_style(
    &s3_setting.bucket,
    region.clone(),
    cred.clone(),
    s3::BucketConfiguration::default(),
  )
  .await
  {
    Ok(_) => Ok(()),
    Err(e) => match e {
      s3::error::S3Error::Http(409, _) => Ok(()), // Bucket already exists
      _ => Err(e),
    },
  }?;

  Ok(s3::Bucket::new(&s3_setting.bucket, region.clone(), cred.clone())?.with_path_style())
}

async fn get_connection_pool(setting: &DatabaseSetting) -> Result<PgPool, Error> {
  info!(
    "Connecting to postgres database with setting: {:?}",
    setting
  );
  PgPoolOptions::new()
    .max_connections(setting.max_connections)
    .acquire_timeout(Duration::from_secs(10))
    .max_lifetime(Duration::from_secs(60 * 60))
    .idle_timeout(Duration::from_secs(60))
    .connect_with(setting.with_db())
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

fn make_ssl_acceptor_builder(certificate: Secret<String>) -> SslAcceptorBuilder {
  let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
  let x509_cert = X509::from_pem(certificate.expose_secret().as_bytes()).unwrap();
  builder.set_certificate(&x509_cert).unwrap();
  builder
    .set_private_key_file("./cert/key.pem", SslFiletype::PEM)
    .unwrap();
  builder
    .set_certificate_chain_file("./cert/cert.pem")
    .unwrap();
  builder
    .set_min_proto_version(Some(openssl::ssl::SslVersion::TLS1_2))
    .unwrap();
  builder
    .set_max_proto_version(Some(openssl::ssl::SslVersion::TLS1_3))
    .unwrap();
  builder
}
