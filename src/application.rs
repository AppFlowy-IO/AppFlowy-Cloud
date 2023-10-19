use crate::component::auth::HEADER_TOKEN;
use crate::config::config::{Config, DatabaseSetting, GoTrueSetting, S3Setting, TlsConfig};
use crate::middleware::cors_mw::default_cors;
use crate::self_signed::create_self_signed_certificate;
use crate::state::AppState;
use actix_identity::IdentityMiddleware;
use actix_session::storage::RedisSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::Key;
use actix_web::{dev::Server, web, web::Data, App, HttpServer};

use actix::Actor;
use actix_web::middleware::Compat;
use anyhow::{Context, Error};
use openssl::ssl::{SslAcceptor, SslAcceptorBuilder, SslFiletype, SslMethod};
use openssl::x509::X509;
use secrecy::{ExposeSecret, Secret};
use snowflake::Snowflake;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::net::TcpListener;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::file_storage::file_storage_scope;
use crate::api::user::user_scope;
use crate::api::workspace::workspace_scope;
use crate::api::ws::ws_scope;
use crate::biz::collab::access_control::{CollabAccessControlImpl, CollabHttpAccessControl};
use crate::biz::collab::storage::init_collab_storage;
use crate::biz::pg_listener::PgListeners;
use crate::biz::workspace::access_control::{
  WorkspaceAccessControlImpl, WorkspaceHttpAccessControl,
};

use crate::middleware::access_control_mw::WorkspaceAccessControl;

use database::file::bucket_s3_impl::S3BucketStorage;
use realtime::client::RealtimeUserImpl;
use realtime::collaborate::CollabServer;
use tracing_actix_web::TracingLogger;

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
      state.workspace_access_control.clone(),
    ))
    .with_acs(CollabHttpAccessControl(state.collab_access_control.clone()));

  let mut server = HttpServer::new(move || {
    App::new()
      .wrap(IdentityMiddleware::default())
      .wrap(
        SessionMiddleware::builder(redis_store.clone(), key.clone())
          .cookie_name(HEADER_TOKEN.to_string())
          .build(),
      )
      .wrap(default_cors())
      .wrap(access_control.clone())
      .wrap(Compat::new(TracingLogger::default()))
      .app_data(web::JsonConfig::default().limit(4096))
      .service(user_scope())
      .service(workspace_scope())
      .service(ws_scope())
      .service(file_storage_scope())
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
  let tls_config = config.application.tls_config.as_ref()?;
  match tls_config {
    TlsConfig::NoTls => None,
    TlsConfig::SelfSigned => Some(create_self_signed_certificate().unwrap()),
  }
}

pub async fn init_state(config: &Config) -> Result<AppState, Error> {
  // Postgres
  let pg_pool = get_connection_pool(&config.database).await?;
  migrate(&pg_pool).await?;

  // Bucket storage
  let s3_bucket = get_aws_s3_bucket(&config.s3).await?;
  let bucket_storage = Arc::new(S3BucketStorage::from_s3_bucket(s3_bucket, pg_pool.clone()));

  // Gotrue
  let gotrue_client = get_gotrue_client(&config.gotrue).await?;
  setup_admin_account(&gotrue_client, &pg_pool, &config.gotrue).await?;

  // Redis
  let redis_client = get_redis_client(config.redis_uri.expose_secret()).await?;

  // Pg listeners
  let pg_listeners = Arc::new(PgListeners::new(&pg_pool).await?);

  // Collab access control
  let collab_member_listener = pg_listeners.subscribe_collab_member_change();
  let collab_access_control = Arc::new(CollabAccessControlImpl::new(
    pg_pool.clone(),
    collab_member_listener,
  ));

  // Workspace access control
  let workspace_member_listener = pg_listeners.subscribe_workspace_member_change();
  let workspace_access_control = Arc::new(WorkspaceAccessControlImpl::new(
    pg_pool.clone(),
    workspace_member_listener,
  ));

  let collab_storage = Arc::new(
    init_collab_storage(
      pg_pool.clone(),
      collab_access_control.clone(),
      workspace_access_control.clone(),
    )
    .await,
  );

  Ok(AppState {
    pg_pool,
    config: Arc::new(config.clone()),
    user: Arc::new(Default::default()),
    id_gen: Arc::new(RwLock::new(Snowflake::new(1))),
    gotrue_client,
    redis_client,
    collab_storage,
    collab_access_control,
    workspace_access_control,
    bucket_storage,
    pg_listeners,
  })
}

async fn setup_admin_account(
  gotrue_client: &gotrue::api::Client,
  pg_pool: &PgPool,
  gotrue_setting: &GoTrueSetting,
) -> Result<(), Error> {
  let admin_email = gotrue_setting.admin_email.as_str();
  let password = gotrue_setting.admin_password.as_str();
  gotrue_client
    .sign_up(admin_email, password)
    .await
    .context("failed to sign-up for admin user")?;

  // Unable to use query! macro here instead
  // because of the auth is a not default schema
  // hopefully this will be fixed in the future
  sqlx::query(
    r#"
      UPDATE auth.users
      SET role = 'supabase_admin', email_confirmed_at = NOW()
      WHERE email = $1
        "#,
  )
  .bind(admin_email)
  .execute(pg_pool)
  .await
  .context("failed to update the admin user")?;
  Ok(())
}

async fn get_redis_client(redis_uri: &str) -> Result<redis::aio::ConnectionManager, Error> {
  let manager = redis::Client::open(redis_uri)
    .context("failed to connect to redis")?
    .get_tokio_connection_manager()
    .await
    .context("failed to get the connection manager")?;
  Ok(manager)
}

async fn get_aws_s3_bucket(s3_setting: &S3Setting) -> Result<s3::Bucket, Error> {
  let region = {
    match s3_setting.use_minio {
      true => s3::Region::Custom {
        region: "".to_owned(),
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
    secret_key: Some(s3_setting.secret_key.to_owned()),
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

// async fn get_aws_s3_client() -> aws_sdk_s3::Client {
//   let credentials = Credentials::new("minioadmin", "minioadmin", None, None, "none");
//   let config = aws_config::SdkConfig::builder()
//     .set_region(Region {})
//     .endpoint_url("http://localhost:9000")
//     .credentials_provider(Some(credentials))
//     .build();
//
//   let client = aws_sdk_s3::Client::new(&config);
//   client
//     .create_bucket()
//     .bucket("hellothisisme")
//     .send()
//     .await
//     .unwrap();
//   client
// }

async fn get_connection_pool(setting: &DatabaseSetting) -> Result<PgPool, Error> {
  PgPoolOptions::new()
    .acquire_timeout(std::time::Duration::from_secs(5))
    .connect_with(setting.with_db())
    .await
    .context("failed to connect to postgres database")
}

async fn migrate(pool: &PgPool) -> Result<(), Error> {
  sqlx::migrate!("./migrations")
    .run(pool)
    .await
    .context("failed to run migrations")
}

async fn get_gotrue_client(setting: &GoTrueSetting) -> Result<gotrue::api::Client, Error> {
  let gotrue_client = gotrue::api::Client::new(reqwest::Client::new(), &setting.base_url);
  gotrue_client
    .health()
    .await
    .context("failed to connect to GoTrue")?;
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
