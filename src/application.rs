use crate::api::metrics::{metrics_registry, metrics_scope};
use crate::biz::casbin::adapter::PgAdapter;
use crate::biz::casbin::MODEL_CONF;
use crate::component::auth::HEADER_TOKEN;
use crate::config::config::{Config, DatabaseSetting, GoTrueSetting, S3Setting};
use crate::middleware::request_id::RequestIdMiddleware;
use crate::self_signed::create_self_signed_certificate;
use crate::state::{AppState, UserCache};
use actix_identity::IdentityMiddleware;
use actix_session::storage::RedisSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::Key;
use actix_web::{dev::Server, web, web::Data, App, HttpServer};

use actix::Actor;
use anyhow::{Context, Error};
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

use crate::api::file_storage::file_storage_scope;
use crate::api::user::user_scope;
use crate::api::workspace::{collab_scope, workspace_scope};
use crate::api::ws::ws_scope;
use crate::biz::casbin::access_control::CasbinAccessControl;
use crate::biz::collab::access_control::CollabHttpAccessControl;
use crate::biz::collab::storage::init_collab_storage;
use crate::biz::pg_listener::PgListeners;
use crate::biz::user::RealtimeUserImpl;
use crate::biz::workspace::access_control::WorkspaceHttpAccessControl;
use crate::middleware::access_control_mw::WorkspaceAccessControl;

use crate::middleware::metrics_mw::MetricsMiddleware;
use casbin::CoreApi;
use database::file::bucket_s3_impl::S3BucketStorage;
use realtime::collaborate::CollabServer;

pub struct Application {
  port: u16,
  server: Server,
}

impl Application {
  pub async fn build(config: Config, state: AppState) -> Result<Self, anyhow::Error> {
    let address = format!("{}:{}", config.application.host, config.application.port);
    let listener = TcpListener::bind(&address)?;
    let port = listener.local_addr().unwrap().port();
    let server = run(listener, state, config).await?;

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

  let storage = state.collab_storage.clone();
  let collab_server = CollabServer::<_, Arc<RealtimeUserImpl>, _>::new(
    storage.clone(),
    state.collab_access_control.clone(),
  )
  .unwrap()
  .start();

  let access_control = WorkspaceAccessControl::new()
    .with_acs(WorkspaceHttpAccessControl(
      state.workspace_access_control.clone().into(),
    ))
    .with_acs(CollabHttpAccessControl(
      state.collab_access_control.clone().into(),
    ));

  // Initialize metrics that which are registered in the registry.
  let (metrics, registry) = metrics_registry();
  let registry_arc = Arc::new(registry);
  let metrics_arc = Arc::new(metrics);

  let mut server = HttpServer::new(move || {
    App::new()
       // Middleware is registered for each App, scope, or Resource and executed in opposite order as registration
      .wrap(MetricsMiddleware)
      .wrap(IdentityMiddleware::default())
      .wrap(
        SessionMiddleware::builder(redis_store.clone(), key.clone())
          .cookie_name(HEADER_TOKEN.to_string())
          .build(),
      )
      // .wrap(DecryptPayloadMiddleware)
      .wrap(RequestIdMiddleware)
      .wrap(access_control.clone())
      .app_data(web::JsonConfig::default().limit(5 * 1024 * 1024))
      .service(user_scope())
      .service(workspace_scope())
      .service(collab_scope())
      .service(ws_scope())
      .service(file_storage_scope())
      .service(metrics_scope())
      .app_data(Data::new(metrics_arc.clone()))
      .app_data(Data::new(registry_arc.clone()))
      .app_data(Data::new(collab_server.clone()))
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

pub async fn init_state(config: &Config) -> Result<AppState, Error> {
  // Postgres
  info!("Preparng to run database migrations...");
  let pg_pool = get_connection_pool(&config.db_settings).await?;
  migrate(&pg_pool).await?;

  // Bucket storage
  info!("Setting up S3 bucket...");
  let s3_bucket = get_aws_s3_bucket(&config.s3).await?;
  let bucket_storage = Arc::new(S3BucketStorage::from_s3_bucket(s3_bucket, pg_pool.clone()));

  // Gotrue
  info!("Connecting to GoTrue...");
  let gotrue_client = get_gotrue_client(&config.gotrue).await?;
  setup_admin_account(&gotrue_client, &pg_pool, &config.gotrue).await?;

  // Redis
  info!("Connecting to Redis...");
  let redis_client = get_redis_client(config.redis_uri.expose_secret()).await?;

  // Pg listeners
  info!("Setting up Pg listeners...");
  let pg_listeners = Arc::new(PgListeners::new(&pg_pool).await?);
  let collab_member_listener = pg_listeners.subscribe_collab_member_change();
  let workspace_member_listener = pg_listeners.subscribe_workspace_member_change();

  info!("Setting up access controls with Casbin...");
  let access_control_model = casbin::DefaultModel::from_str(MODEL_CONF).await?;
  let access_control_adapter = PgAdapter::new(pg_pool.clone());
  let enforcer = casbin::Enforcer::new(access_control_model, access_control_adapter).await?;
  let casbin_access_control = CasbinAccessControl::new(
    pg_pool.clone(),
    collab_member_listener,
    workspace_member_listener,
    enforcer,
  );

  let collab_access_control = casbin_access_control.new_collab_access_control();
  let workspace_access_control = casbin_access_control.new_workspace_access_control();

  let collab_storage = Arc::new(
    init_collab_storage(
      pg_pool.clone(),
      redis_client.clone(),
      collab_access_control.clone(),
      workspace_access_control.clone(),
    )
    .await,
  );
  let users = UserCache::new(pg_pool.clone());

  info!("Application state initialized");
  Ok(AppState {
    pg_pool,
    config: Arc::new(config.clone()),
    users: Arc::new(users),
    id_gen: Arc::new(RwLock::new(Snowflake::new(1))),
    gotrue_client,
    redis_client,
    collab_storage,
    collab_access_control,
    workspace_access_control,
    bucket_storage,
    pg_listeners,
    casbin_access_control,
  })
}

async fn setup_admin_account(
  gotrue_client: &gotrue::api::Client,
  pg_pool: &PgPool,
  gotrue_setting: &GoTrueSetting,
) -> Result<(), Error> {
  let admin_email = gotrue_setting.admin_email.as_str();
  let password = gotrue_setting.admin_password.as_str();
  let res_resp = gotrue_client.sign_up(admin_email, password).await;
  match res_resp {
    Err(err) => {
      if let app_error::gotrue::GoTrueError::Internal(err) = err {
        match (err.code, err.msg.as_str()) {
          (400, "User already registered") => {
            info!("Admin user already registered");
            Ok(())
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
          Ok(())
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

          Ok(())
        },
      }
    },
  }
}

async fn get_redis_client(redis_uri: &str) -> Result<redis::aio::ConnectionManager, Error> {
  info!("Connecting to redis with uri: {}", redis_uri);
  let manager = redis::Client::open(redis_uri)
    .context("failed to connect to redis")?
    .get_tokio_connection_manager()
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
    .connect_with(setting.with_db())
    .await
    .map_err(|e| anyhow::anyhow!("Failed to connect to postgres database: {}", e))
}

async fn migrate(pool: &PgPool) -> Result<(), Error> {
  sqlx::migrate!("./migrations")
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
